// Parity runner: executes .scn scenarios with the C++ SoLoud reference and
// writes <name>.f32 (interleaved little-endian samples from every mix) and
// <name>.log (one line per step that returns a value) for each.
//
// Usage: parity_runner <output dir> <scenario.scn>...
//
// The format is specified in tests/scenarios/README.md. Keep the accepted
// operations and checks identical to crates/solouder-parity/src/run.rs.

#include <cerrno>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iterator>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "soloud.h"
#include "soloud_bassboostfilter.h"
#include "soloud_biquadresonantfilter.h"
#include "soloud_bus.h"
#include "soloud_dcremovalfilter.h"
#include "soloud_echofilter.h"
#include "soloud_fft.h"
#include "soloud_fftfilter.h"
#include "soloud_flangerfilter.h"
#include "soloud_freeverbfilter.h"
#include "soloud_lofifilter.h"
#include "soloud_queue.h"
#include "soloud_robotizefilter.h"
#include "soloud_waveshaperfilter.h"
#include "soloud_wav.h"
#include "soloud_wavstream.h"

namespace
{
	const unsigned int MAX_MIX_FRAMES = 4096;

	struct Step
	{
		int line;
		std::string op;
		std::vector<std::pair<std::string, std::string> > args;

		const std::string *get(const std::string &key) const
		{
			for (size_t i = 0; i < args.size(); i++)
				if (args[i].first == key)
					return &args[i].second;
			return 0;
		}
	};

	struct ScenarioError : std::runtime_error
	{
		int line;
		ScenarioError(int aLine, const std::string &aMessage) : std::runtime_error(aMessage), line(aLine) {}
	};

	std::string trim(const std::string &s)
	{
		const char *space = " \t\r\n";
		size_t begin = s.find_first_not_of(space);
		if (begin == std::string::npos)
			return "";
		return s.substr(begin, s.find_last_not_of(space) - begin + 1);
	}

	std::vector<Step> parse(std::istream &in)
	{
		std::vector<Step> steps;
		std::string raw;
		int line = 0;
		while (std::getline(in, raw))
		{
			line++;
			std::string content = trim(raw);
			if (content.empty() || content[0] == '#')
				continue;
			std::istringstream tokens(content);
			Step step;
			step.line = line;
			tokens >> step.op;
			std::string token;
			while (tokens >> token)
			{
				size_t eq = token.find('=');
				if (eq == std::string::npos || eq == 0)
					throw ScenarioError(line, "expected key=value, got `" + token + "`");
				std::string key = token.substr(0, eq);
				if (step.get(key))
					throw ScenarioError(line, "duplicate argument `" + key + "`");
				step.args.push_back(std::make_pair(key, token.substr(eq + 1)));
			}
			steps.push_back(step);
		}
		return steps;
	}

	void checkKeys(const Step &step, const char *const *allowed, size_t count)
	{
		for (size_t i = 0; i < step.args.size(); i++)
		{
			bool known = false;
			for (size_t k = 0; k < count; k++)
				known = known || step.args[i].first == allowed[k];
			if (!known)
				throw ScenarioError(step.line, "unknown argument `" + step.args[i].first + "` for `" + step.op + "`");
		}
	}

	unsigned int uintArg(const Step &step, const char *key, bool required, unsigned int fallback)
	{
		const std::string *raw = step.get(key);
		if (!raw)
		{
			if (required)
				throw ScenarioError(step.line, "`" + step.op + "` needs `" + key + "=`");
			return fallback;
		}
		const char *text = raw->c_str();
		char *end = 0;
		errno = 0;
		unsigned long value = std::strtoul(text, &end, 10);
		if (raw->empty() || *end != 0 || errno != 0 || (*text < '0' || *text > '9') || value > 0xffffffffUL)
			throw ScenarioError(step.line, "invalid value `" + *raw + "` for `" + key + "`");
		return (unsigned int)value;
	}

	// Decimal numbers only (-?digits[.digits]); the Rust runner accepts the
	// same grammar. Returns 0 when the key is absent.
	const std::string *decimalArg(const Step &step, const char *key)
	{
		const std::string *raw = step.get(key);
		if (!raw)
			return 0;
		const std::string &text = *raw;
		size_t i = (!text.empty() && text[0] == '-') ? 1 : 0;
		size_t digits = 0;
		while (i < text.size() && text[i] >= '0' && text[i] <= '9')
		{
			i++;
			digits++;
		}
		bool valid = digits > 0;
		if (valid && i < text.size() && text[i] == '.')
		{
			i++;
			size_t fraction = 0;
			while (i < text.size() && text[i] >= '0' && text[i] <= '9')
			{
				i++;
				fraction++;
			}
			valid = fraction > 0;
		}
		if (!valid || i != text.size())
			throw ScenarioError(step.line, "invalid number `" + text + "` for `" + key + "`");
		return raw;
	}

	float floatArg(const Step &step, const char *key, float fallback)
	{
		const std::string *raw = decimalArg(step, key);
		return raw ? std::strtof(raw->c_str(), 0) : fallback;
	}

	// Times in seconds, parsed as double like SoLoud's `time`.
	double timeArg(const Step &step, const char *key, double fallback)
	{
		const std::string *raw = decimalArg(step, key);
		return raw ? std::strtod(raw->c_str(), 0) : fallback;
	}

	bool flagArg(const Step &step, const char *key, bool fallback)
	{
		const std::string *raw = step.get(key);
		if (!raw)
			return fallback;
		if (*raw == "0")
			return false;
		if (*raw == "1")
			return true;
		throw ScenarioError(step.line, "invalid flag `" + *raw + "` for `" + key + "` (0 or 1)");
	}

	const std::string &textArg(const Step &step, const char *key)
	{
		const std::string *raw = step.get(key);
		if (!raw)
			throw ScenarioError(step.line, "`" + step.op + "` needs `" + key + "=`");
		return *raw;
	}

	std::string hex32(float aValue)
	{
		unsigned int bits;
		std::memcpy(&bits, &aValue, 4);
		char text[16];
		std::snprintf(text, sizeof(text), "%08x", bits);
		return text;
	}

	std::string hex64(double aValue)
	{
		unsigned long long bits;
		std::memcpy(&bits, &aValue, 8);
		char text[24];
		std::snprintf(text, sizeof(text), "%016llx", bits);
		return text;
	}

	const unsigned int MAX_SOUND_SAMPLES = 1u << 22;

	// Test signals; keep identical to crates/solouder-parity/src/pattern.rs.
	std::vector<float> renderPattern(const Step &step, unsigned int channels, unsigned int frames, float amp)
	{
		const std::string *raw = step.get("pattern");
		std::string pattern = raw ? *raw : "dc";
		unsigned int period = 0, state = 0;
		if (pattern == "saw")
		{
			period = uintArg(step, "period", false, 100);
			if (period == 0)
				throw ScenarioError(step.line, "`period` must be at least 1");
		}
		else if (pattern == "noise")
		{
			state = uintArg(step, "seed", false, 1);
			if (state == 0)
				throw ScenarioError(step.line, "`seed` must be at least 1");
		}
		else if (pattern != "dc")
		{
			throw ScenarioError(step.line, "unknown pattern `" + pattern + "`");
		}

		std::vector<float> samples;
		samples.reserve((size_t)channels * frames);
		for (unsigned int c = 0; c < channels; c++)
		{
			for (unsigned int f = 0; f < frames; f++)
			{
				float value = 1.0f;
				if (pattern == "saw")
				{
					value = (float)((f + c * 7) % period) / (float)period * 2.0f - 1.0f;
				}
				else if (pattern == "noise")
				{
					state ^= state << 13;
					state ^= state >> 17;
					state ^= state << 5;
					value = (float)(state >> 8) / 8388608.0f - 1.0f;
				}
				samples.push_back(value * amp);
			}
		}
		return samples;
	}

	// Colliders and attenuators that `sound3d` selects by name. Keep identical
	// to crates/solouder-parity/src/spatial_custom.rs.

	// A wall at x = user data / 10: sources past it are heard at a quarter.
	struct WallCollider : SoLoud::AudioCollider
	{
		virtual float collide(SoLoud::Soloud *, SoLoud::AudioSourceInstance3dData *aData, int aUserData)
		{
			return aData->m3dPosition[0] > (float)aUserData / 10.0f ? 0.25f : 1.0f;
		}
	};

	// Quieter the higher or lower the source: 1 / (1 + |y| * user data / 100).
	struct ShellCollider : SoLoud::AudioCollider
	{
		virtual float collide(SoLoud::Soloud *, SoLoud::AudioSourceInstance3dData *aData, int aUserData)
		{
			float y = aData->m3dPosition[1];
			if (y < 0)
				y = -y;
			return 1.0f / (1.0f + y * (float)aUserData / 100.0f);
		}
	};

	// Inverse square of the distance clamped to the minimum and maximum,
	// times the rolloff factor.
	struct SquareAttenuator : SoLoud::AudioAttenuator
	{
		virtual float attenuate(float aDistance, float aMinDistance, float aMaxDistance, float aRolloffFactor)
		{
			float d = aDistance > aMinDistance ? aDistance : aMinDistance;
			d = d < aMaxDistance ? d : aMaxDistance;
			return aMinDistance * aMinDistance / (d * d) * aRolloffFactor;
		}
	};

	// Full volume closer than the maximum distance, silent from there.
	struct CutoffAttenuator : SoLoud::AudioAttenuator
	{
		virtual float attenuate(float aDistance, float, float aMaxDistance, float)
		{
			return aDistance < aMaxDistance ? 1.0f : 0.0f;
		}
	};

	struct Runner
	{
		SoLoud::Soloud *soloud;
		bool initialized;
		std::map<std::string, SoLoud::Wav *> sounds;
		std::map<std::string, SoLoud::WavStream *> streams;
		std::map<std::string, SoLoud::Bus *> buses;
		std::map<std::string, SoLoud::Queue *> queues;
		std::map<std::string, SoLoud::handle> handles;
		// Filters by name. Sounds keep pointers to their filters, so every
		// filter created lives until the runner ends; not cleared by init.
		std::map<std::string, SoLoud::Filter *> filters;
		std::vector<SoLoud::Filter *> ownedFilters;
		std::vector<float> samples;
		std::vector<std::string> log;
		// tests/fixtures/, which `file=` paths are relative to.
		std::string fixtureDir;

		explicit Runner(const std::string &aFixtureDir) : soloud(0), initialized(false), fixtureDir(aFixtureDir) {}

		~Runner()
		{
			clearSounds();
			delete soloud;
			for (size_t i = 0; i < ownedFilters.size(); i++)
				delete ownedFilters[i];
		}

		// A Wav, Bus or Queue stops its voices in the engine when destroyed,
		// so they go before the engine. Queues go first: their queued sounds
		// read the Wavs they were made from.
		void clearSounds()
		{
			for (std::map<std::string, SoLoud::Queue *>::iterator it = queues.begin(); it != queues.end(); ++it)
				delete it->second;
			queues.clear();
			for (std::map<std::string, SoLoud::Wav *>::iterator it = sounds.begin(); it != sounds.end(); ++it)
				delete it->second;
			sounds.clear();
			for (std::map<std::string, SoLoud::WavStream *>::iterator it = streams.begin(); it != streams.end(); ++it)
				delete it->second;
			streams.clear();
			for (std::map<std::string, SoLoud::Bus *>::iterator it = buses.begin(); it != buses.end(); ++it)
				delete it->second;
			buses.clear();
			handles.clear();
		}

		SoLoud::Soloud &engine(const Step &step)
		{
			if (!initialized)
				throw ScenarioError(step.line, "`" + step.op + "` needs a successful init first");
			return *soloud;
		}

		SoLoud::handle handleArg(const Step &step)
		{
			const std::string &name = textArg(step, "h");
			std::map<std::string, SoLoud::handle>::iterator it = handles.find(name);
			if (it == handles.end())
				throw ScenarioError(step.line, "no handle named `" + name + "`");
			return it->second;
		}

		SoLoud::handle namedHandle(const Step &step, const char *key)
		{
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::handle>::iterator it = handles.find(name);
			if (it == handles.end())
				throw ScenarioError(step.line, "no handle named `" + name + "`");
			return it->second;
		}

		SoLoud::Wav &soundArg(const Step &step, const char *key)
		{
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::Wav *>::iterator it = sounds.find(name);
			if (it == sounds.end())
				throw ScenarioError(step.line, "no sound named `" + name + "`");
			return *it->second;
		}

		SoLoud::Bus &busArg(const Step &step, const char *key)
		{
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::Bus *>::iterator it = buses.find(name);
			if (it == buses.end())
				throw ScenarioError(step.line, "no bus named `" + name + "`");
			return *it->second;
		}

		SoLoud::Queue &queueArg(const Step &step, const char *key)
		{
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::Queue *>::iterator it = queues.find(name);
			if (it == queues.end())
				throw ScenarioError(step.line, "no queue named `" + name + "`");
			return *it->second;
		}

		// A sound, else a stream, by name: what a queue can hold.
		SoLoud::AudioSource &queuedArg(const Step &step, const char *key)
		{
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::WavStream *>::iterator stream = streams.find(name);
			if (!sounds.count(name) && stream != streams.end())
				return *stream->second;
			return soundArg(step, key);
		}

		// A sound, else a stream, else a bus, else a queue, by name.
		SoLoud::AudioSource &sourceArg(const Step &step, const char *key)
		{
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::Wav *>::iterator it = sounds.find(name);
			if (it != sounds.end())
				return *it->second;
			std::map<std::string, SoLoud::WavStream *>::iterator stream = streams.find(name);
			if (stream != streams.end())
				return *stream->second;
			std::map<std::string, SoLoud::Bus *>::iterator bus = buses.find(name);
			if (bus != buses.end())
				return *bus->second;
			std::map<std::string, SoLoud::Queue *>::iterator queue = queues.find(name);
			if (queue == queues.end())
				throw ScenarioError(step.line, "no sound, stream, bus or queue named `" + name + "`");
			return *queue->second;
		}

		// The filter named by `key`, or null when the key is absent.
		SoLoud::Filter *filterArg(const Step &step, const char *key)
		{
			if (!step.get(key))
				return 0;
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::Filter *>::iterator it = filters.find(name);
			if (it == filters.end())
				throw ScenarioError(step.line, "no filter named `" + name + "`");
			return it->second;
		}

		int biquadType(const Step &step)
		{
			const std::string *raw = step.get("type");
			if (!raw || *raw == "lowpass")
				return SoLoud::BiquadResonantFilter::LOWPASS;
			if (*raw == "highpass")
				return SoLoud::BiquadResonantFilter::HIGHPASS;
			if (*raw == "bandpass")
				return SoLoud::BiquadResonantFilter::BANDPASS;
			throw ScenarioError(step.line, "unknown biquad type `" + *raw + "`");
		}

		// `filter`: a new filter of `kind`. When any of the kind's settings is
		// given, they go to its setParams; missing ones take the defaults.
		void createFilter(const Step &step)
		{
			const std::string &kind = textArg(step, "kind");
			SoLoud::Filter *filter = 0;
			SoLoud::result code = SoLoud::SO_NO_ERROR;
			if (kind == "biquad")
			{
				static const char *const keys[] = { "name", "kind", "type", "freq", "res" };
				checkKeys(step, keys, 5);
				textArg(step, "name");
				int type = biquadType(step);
				float freq = floatArg(step, "freq", 1000.0f);
				float res = floatArg(step, "res", 2.0f);
				SoLoud::BiquadResonantFilter *f = new SoLoud::BiquadResonantFilter;
				if (step.get("type") || step.get("freq") || step.get("res"))
					code = f->setParams(type, freq, res);
				filter = f;
			}
			else if (kind == "lofi")
			{
				static const char *const keys[] = { "name", "kind", "rate", "bits" };
				checkKeys(step, keys, 4);
				textArg(step, "name");
				float rate = floatArg(step, "rate", 4000.0f);
				float bits = floatArg(step, "bits", 3.0f);
				SoLoud::LofiFilter *f = new SoLoud::LofiFilter;
				if (step.get("rate") || step.get("bits"))
					code = f->setParams(rate, bits);
				filter = f;
			}
			else if (kind == "echo")
			{
				static const char *const keys[] = { "name", "kind", "delay", "decay", "filter" };
				checkKeys(step, keys, 5);
				textArg(step, "name");
				float delay = floatArg(step, "delay", 0.3f);
				float decay = floatArg(step, "decay", 0.7f);
				float lowpass = floatArg(step, "filter", 0.0f);
				SoLoud::EchoFilter *f = new SoLoud::EchoFilter;
				if (step.get("delay") || step.get("decay") || step.get("filter"))
					code = f->setParams(delay, decay, lowpass);
				filter = f;
			}
			else if (kind == "waveshaper")
			{
				static const char *const keys[] = { "name", "kind", "amount" };
				checkKeys(step, keys, 3);
				textArg(step, "name");
				float amount = floatArg(step, "amount", 0.0f);
				SoLoud::WaveShaperFilter *f = new SoLoud::WaveShaperFilter;
				if (step.get("amount"))
					code = f->setParams(amount);
				filter = f;
			}
			else if (kind == "dc_removal")
			{
				static const char *const keys[] = { "name", "kind", "length" };
				checkKeys(step, keys, 3);
				textArg(step, "name");
				float length = floatArg(step, "length", 0.1f);
				SoLoud::DCRemovalFilter *f = new SoLoud::DCRemovalFilter;
				if (step.get("length"))
					code = f->setParams(length);
				filter = f;
			}
			else if (kind == "fft")
			{
				static const char *const keys[] = { "name", "kind" };
				checkKeys(step, keys, 2);
				textArg(step, "name");
				filter = new SoLoud::FFTFilter;
			}
			else if (kind == "bassboost")
			{
				static const char *const keys[] = { "name", "kind", "boost" };
				checkKeys(step, keys, 3);
				textArg(step, "name");
				float boost = floatArg(step, "boost", 2.0f);
				SoLoud::BassboostFilter *f = new SoLoud::BassboostFilter;
				if (step.get("boost"))
					code = f->setParams(boost);
				filter = f;
			}
			else if (kind == "robotize")
			{
				static const char *const keys[] = { "name", "kind", "freq", "wave" };
				checkKeys(step, keys, 4);
				textArg(step, "name");
				float freq = floatArg(step, "freq", 30.0f);
				unsigned int wave = uintArg(step, "wave", false, 0);
				SoLoud::RobotizeFilter *f = new SoLoud::RobotizeFilter;
				if (step.get("freq") || step.get("wave"))
					f->setParams(freq, (int)wave);
				filter = f;
			}
			else if (kind == "flanger")
			{
				static const char *const keys[] = { "name", "kind", "delay", "freq" };
				checkKeys(step, keys, 4);
				textArg(step, "name");
				float delay = floatArg(step, "delay", 0.005f);
				float freq = floatArg(step, "freq", 10.0f);
				SoLoud::FlangerFilter *f = new SoLoud::FlangerFilter;
				if (step.get("delay") || step.get("freq"))
					code = f->setParams(delay, freq);
				filter = f;
			}
			else if (kind == "freeverb")
			{
				static const char *const keys[] = { "name", "kind", "freeze", "room", "damp", "width" };
				checkKeys(step, keys, 6);
				textArg(step, "name");
				float freeze = floatArg(step, "freeze", 0.0f);
				float room = floatArg(step, "room", 0.5f);
				float damp = floatArg(step, "damp", 0.5f);
				float width = floatArg(step, "width", 1.0f);
				SoLoud::FreeverbFilter *f = new SoLoud::FreeverbFilter;
				if (step.get("freeze") || step.get("room") || step.get("damp") || step.get("width"))
					code = f->setParams(freeze, room, damp, width);
				filter = f;
			}
			else
			{
				throw ScenarioError(step.line, "unknown filter kind `" + kind + "`");
			}
			const std::string &name = textArg(step, "name");
			ownedFilters.push_back(filter);
			filters[name] = filter;
			std::ostringstream line;
			line << "filter " << name << " " << code;
			logLine(line);
		}

		// `fft`: transforms `size` floats of a test signal and appends the
		// result to the samples.
		void fftDump(const Step &step)
		{
			static const char *const keys[] = { "size", "inverse", "pattern", "amp", "period", "seed" };
			checkKeys(step, keys, 6);
			unsigned int size = uintArg(step, "size", true, 0);
			if (size < 4 || size > 4096 || (size & (size - 1)) != 0)
				throw ScenarioError(step.line, "`size` must be a power of two from 4 to 4096");
			bool inverse = flagArg(step, "inverse", false);
			float amp = floatArg(step, "amp", 1.0f);
			std::vector<float> buffer = renderPattern(step, 1, size, amp);
			if (inverse)
				SoLoud::FFT::ifft(&buffer[0], size);
			else
				SoLoud::FFT::fft(&buffer[0], size);
			samples.insert(samples.end(), buffer.begin(), buffer.end());
		}

		// `filter_set`, `filter_fade`, `filter_oscillate` and `filter_query`;
		// without `h` they address the global filters (handle 0).
		void filterParameter(const Step &step)
		{
			static const char *const setKeys[] = { "h", "slot", "param", "value" };
			static const char *const fadeKeys[] = { "h", "slot", "param", "to", "time" };
			static const char *const oscillateKeys[] = { "h", "slot", "param", "from", "to", "time" };
			static const char *const queryKeys[] = { "h", "slot", "param" };
			if (step.op == "filter_set")
				checkKeys(step, setKeys, 4);
			else if (step.op == "filter_fade")
				checkKeys(step, fadeKeys, 5);
			else if (step.op == "filter_oscillate")
				checkKeys(step, oscillateKeys, 6);
			else
				checkKeys(step, queryKeys, 3);
			SoLoud::handle h = step.get("h") ? handleArg(step) : 0;
			unsigned int slot = uintArg(step, "slot", true, 0);
			unsigned int param = uintArg(step, "param", true, 0);
			SoLoud::Soloud &s = engine(step);
			if (step.op == "filter_set")
			{
				textArg(step, "value");
				s.setFilterParameter(h, slot, param, floatArg(step, "value", 0.0f));
			}
			else if (step.op == "filter_fade")
			{
				textArg(step, "to");
				textArg(step, "time");
				s.fadeFilterParameter(h, slot, param, floatArg(step, "to", 0.0f), timeArg(step, "time", 0));
			}
			else if (step.op == "filter_oscillate")
			{
				textArg(step, "from");
				textArg(step, "to");
				textArg(step, "time");
				s.oscillateFilterParameter(h, slot, param, floatArg(step, "from", 0.0f), floatArg(step, "to", 0.0f),
					timeArg(step, "time", 0));
			}
			else
			{
				std::ostringstream line;
				line << "filter_query " << (step.get("h") ? textArg(step, "h") : std::string("global")) << " " << slot << " "
					<< param << " " << hex32(s.getFilterParameter(h, slot, param));
				logLine(line);
			}
		}

		void logLine(const std::ostringstream &line)
		{
			log.push_back(line.str());
		}

		// The fixture named by `file=`: letters, digits, '.', '_', '-' and '/'
		// separators, without ".." or a leading '/'.
		std::string fixtureArg(const Step &step)
		{
			const std::string &file = textArg(step, "file");
			bool valid = !file.empty() && file[0] != '/';
			size_t partStart = 0;
			for (size_t i = 0; valid && i <= file.size(); i++)
			{
				if (i == file.size() || file[i] == '/')
				{
					std::string part = file.substr(partStart, i - partStart);
					valid = !part.empty() && part != "..";
					partStart = i + 1;
				}
				else
				{
					char c = file[i];
					valid = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
						c == '.' || c == '_' || c == '-';
				}
			}
			if (!valid)
				throw ScenarioError(step.line, "invalid fixture path `" + file + "`");
			std::string path = fixtureDir + "/" + file;
			std::ifstream probe(path.c_str(), std::ios::binary);
			if (!probe)
				throw ScenarioError(step.line, "cannot read " + path);
			return path;
		}

		void storeSound(const std::string &name, SoLoud::Wav *wav, SoLoud::result code, float volume, bool looping)
		{
			if (code != SoLoud::SO_NO_ERROR)
			{
				delete wav;
				return;
			}
			wav->setVolume(volume);
			wav->setLooping(looping);
			std::map<std::string, SoLoud::Wav *>::iterator old = sounds.find(name);
			if (old != sounds.end())
				delete old->second;
			sounds[name] = wav;
		}

		// Whether all of the keys are given; some but not all is an error.
		bool groupArg(const Step &step, const char *const *keys, size_t count, const char *what)
		{
			size_t given = 0;
			for (size_t i = 0; i < count; i++)
				given += step.get(keys[i]) ? 1 : 0;
			if (given != 0 && given != count)
				throw ScenarioError(step.line, std::string(what) + " go together");
			return given == count;
		}

		// `sound3d collider=`: a collider by name, or null for `none`.
		SoLoud::AudioCollider *colliderArg(const Step &step)
		{
			static WallCollider wall;
			static ShellCollider shell;
			const std::string &name = textArg(step, "collider");
			if (name == "wall")
				return &wall;
			if (name == "shell")
				return &shell;
			if (name == "none")
				return 0;
			throw ScenarioError(step.line, "unknown collider `" + name + "`");
		}

		// `sound3d attenuator=`: an attenuator by name, or null for `none`.
		SoLoud::AudioAttenuator *attenuatorArg(const Step &step)
		{
			static SquareAttenuator square;
			static CutoffAttenuator cutoff;
			const std::string &name = textArg(step, "attenuator");
			if (name == "square")
				return &square;
			if (name == "cutoff")
				return &cutoff;
			if (name == "none")
				return 0;
			throw ScenarioError(step.line, "unknown attenuator `" + name + "`");
		}

		// `sound3d`, `play3d`, `listener`, `set3d`, `update3d`, `speaker` and
		// `sound_speed`.
		void spatialOp(const Step &step)
		{
			static const char *const xyz[] = { "x", "y", "z" };
			static const char *const vel[] = { "vx", "vy", "vz" };
			static const char *const at[] = { "ax", "ay", "az" };
			static const char *const up[] = { "ux", "uy", "uz" };
			static const char *const distance[] = { "min", "max" };
			static const char *const attenuation[] = { "model", "rolloff" };
			if (step.op == "sound3d")
			{
				static const char *const keys[] = { "name", "min", "max", "model", "rolloff", "relative", "doppler", "delay",
					"collider", "data", "attenuator" };
				checkKeys(step, keys, 11);
				SoLoud::AudioSource &source = sourceArg(step, "name");
				bool hasDistance = groupArg(step, distance, 2, "`min` and `max`");
				bool hasModel = groupArg(step, attenuation, 2, "`model` and `rolloff`");
				unsigned int model = uintArg(step, "model", false, 0);
				bool relative = flagArg(step, "relative", false);
				float doppler = floatArg(step, "doppler", 1.0f);
				bool delay = flagArg(step, "delay", false);
				if (step.get("data") && !step.get("collider"))
					throw ScenarioError(step.line, "`data` needs `collider`");
				SoLoud::AudioCollider *collider = step.get("collider") ? colliderArg(step) : 0;
				int data = (int)uintArg(step, "data", false, 0);
				SoLoud::AudioAttenuator *attenuator = step.get("attenuator") ? attenuatorArg(step) : 0;
				engine(step);
				if (hasDistance)
					source.set3dMinMaxDistance(floatArg(step, "min", 0.0f), floatArg(step, "max", 0.0f));
				if (hasModel)
					source.set3dAttenuation(model, floatArg(step, "rolloff", 0.0f));
				if (step.get("relative"))
					source.set3dListenerRelative(relative);
				if (step.get("doppler"))
					source.set3dDopplerFactor(doppler);
				if (step.get("delay"))
					source.set3dDistanceDelay(delay);
				if (step.get("collider"))
					source.set3dCollider(collider, data);
				if (step.get("attenuator"))
					source.set3dAttenuator(attenuator);
			}
			else if (step.op == "play3d")
			{
				static const char *const keys[] = { "sound", "as", "x", "y", "z", "vx", "vy", "vz", "volume", "paused", "via",
					"bus", "clock" };
				checkKeys(step, keys, 13);
				if (step.get("clock") && step.get("paused"))
					throw ScenarioError(step.line, "`clock` and `paused` cannot be combined");
				if (step.get("bus") && step.get("via"))
					throw ScenarioError(step.line, "`bus` and `via` cannot be combined");
				SoLoud::AudioSource &source = sourceArg(step, "sound");
				SoLoud::Bus *through = step.get("bus") ? &busArg(step, "bus") : 0;
				SoLoud::handle via = step.get("via") ? namedHandle(step, "via") : 0;
				std::string name = textArg(step, "as");
				float x = floatArg(step, "x", 0.0f), y = floatArg(step, "y", 0.0f), z = floatArg(step, "z", 0.0f);
				float vx = floatArg(step, "vx", 0.0f), vy = floatArg(step, "vy", 0.0f), vz = floatArg(step, "vz", 0.0f);
				float volume = floatArg(step, "volume", 1.0f);
				bool paused = flagArg(step, "paused", false);
				double clock = timeArg(step, "clock", 0);
				SoLoud::Soloud &s = engine(step);
				SoLoud::handle h;
				if (through && step.get("clock"))
					h = through->play3dClocked(clock, source, x, y, z, vx, vy, vz, volume);
				else if (through)
					h = through->play3d(source, x, y, z, vx, vy, vz, volume, paused);
				else if (step.get("clock"))
					h = s.play3dClocked(clock, source, x, y, z, vx, vy, vz, volume, via);
				else
					h = s.play3d(source, x, y, z, vx, vy, vz, volume, paused, via);
				handles[name] = h;
				std::ostringstream line;
				line << "play3d " << name << " " << h;
				logLine(line);
			}
			else if (step.op == "listener")
			{
				static const char *const keys[] = { "x", "y", "z", "ax", "ay", "az", "ux", "uy", "uz", "vx", "vy", "vz", "params" };
				checkKeys(step, keys, 13);
				bool hasPosition = groupArg(step, xyz, 3, "`x`, `y` and `z`");
				bool hasAt = groupArg(step, at, 3, "`ax`, `ay` and `az`");
				bool hasUp = groupArg(step, up, 3, "`ux`, `uy` and `uz`");
				bool hasVelocity = groupArg(step, vel, 3, "`vx`, `vy` and `vz`");
				bool params = flagArg(step, "params", false);
				if (params && !(hasPosition && hasAt && hasUp))
					throw ScenarioError(step.line, "`params=1` needs the position, `at` and `up`");
				if (!hasPosition && !hasAt && !hasUp && !hasVelocity)
					throw ScenarioError(step.line, "`listener` needs a position, `at`, `up` or a velocity");
				float x = floatArg(step, "x", 0.0f), y = floatArg(step, "y", 0.0f), z = floatArg(step, "z", 0.0f);
				float ax = floatArg(step, "ax", 0.0f), ay = floatArg(step, "ay", 0.0f), az = floatArg(step, "az", 0.0f);
				float ux = floatArg(step, "ux", 0.0f), uy = floatArg(step, "uy", 0.0f), uz = floatArg(step, "uz", 0.0f);
				float vx = floatArg(step, "vx", 0.0f), vy = floatArg(step, "vy", 0.0f), vz = floatArg(step, "vz", 0.0f);
				SoLoud::Soloud &s = engine(step);
				if (params)
				{
					s.set3dListenerParameters(x, y, z, ax, ay, az, ux, uy, uz, vx, vy, vz);
					return;
				}
				if (hasPosition)
					s.set3dListenerPosition(x, y, z);
				if (hasAt)
					s.set3dListenerAt(ax, ay, az);
				if (hasUp)
					s.set3dListenerUp(ux, uy, uz);
				if (hasVelocity)
					s.set3dListenerVelocity(vx, vy, vz);
			}
			else if (step.op == "set3d")
			{
				static const char *const keys[] = { "h", "x", "y", "z", "vx", "vy", "vz", "params", "min", "max", "model", "rolloff",
					"doppler" };
				checkKeys(step, keys, 13);
				SoLoud::handle h = handleArg(step);
				bool hasPosition = groupArg(step, xyz, 3, "`x`, `y` and `z`");
				bool hasVelocity = groupArg(step, vel, 3, "`vx`, `vy` and `vz`");
				bool hasDistance = groupArg(step, distance, 2, "`min` and `max`");
				bool hasModel = groupArg(step, attenuation, 2, "`model` and `rolloff`");
				bool params = flagArg(step, "params", false);
				if (params && !hasPosition)
					throw ScenarioError(step.line, "`params=1` needs `x`, `y` and `z`");
				unsigned int model = uintArg(step, "model", false, 0);
				float doppler = floatArg(step, "doppler", 1.0f);
				SoLoud::Soloud &s = engine(step);
				float x = floatArg(step, "x", 0.0f), y = floatArg(step, "y", 0.0f), z = floatArg(step, "z", 0.0f);
				float vx = floatArg(step, "vx", 0.0f), vy = floatArg(step, "vy", 0.0f), vz = floatArg(step, "vz", 0.0f);
				if (params)
				{
					s.set3dSourceParameters(h, x, y, z, vx, vy, vz);
				}
				else
				{
					if (hasPosition)
						s.set3dSourcePosition(h, x, y, z);
					if (hasVelocity)
						s.set3dSourceVelocity(h, vx, vy, vz);
				}
				if (hasDistance)
					s.set3dSourceMinMaxDistance(h, floatArg(step, "min", 0.0f), floatArg(step, "max", 0.0f));
				if (hasModel)
					s.set3dSourceAttenuation(h, model, floatArg(step, "rolloff", 0.0f));
				if (step.get("doppler"))
					s.set3dSourceDopplerFactor(h, doppler);
			}
			else if (step.op == "update3d")
			{
				checkKeys(step, 0, 0);
				engine(step).update3dAudio();
			}
			else if (step.op == "sound_speed")
			{
				static const char *const keys[] = { "speed" };
				checkKeys(step, keys, 1);
				SoLoud::Soloud &s = engine(step);
				std::ostringstream line;
				line << "sound_speed";
				if (step.get("speed"))
					line << " set=" << s.set3dSoundSpeed(floatArg(step, "speed", 0.0f));
				line << " get=" << hex32(s.get3dSoundSpeed());
				logLine(line);
			}
			else
			{
				static const char *const keys[] = { "channel", "x", "y", "z" };
				checkKeys(step, keys, 4);
				unsigned int channel = uintArg(step, "channel", true, 0);
				bool set = groupArg(step, xyz, 3, "`x`, `y` and `z`");
				SoLoud::Soloud &s = engine(step);
				std::ostringstream line;
				line << "speaker " << channel;
				if (set)
					line << " set=" << s.setSpeakerPosition(channel, floatArg(step, "x", 0.0f), floatArg(step, "y", 0.0f), floatArg(step, "z", 0.0f));
				float x = 0.0f, y = 0.0f, z = 0.0f;
				SoLoud::result code = s.getSpeakerPosition(channel, x, y, z);
				line << " get=" << code << " " << hex32(x) << " " << hex32(y) << " " << hex32(z);
				logLine(line);
			}
		}

		void queueOp(const Step &step)
		{
			if (step.op == "queue")
			{
				static const char *const keys[] = { "name", "channels", "rate", "from", "volume" };
				checkKeys(step, keys, 5);
				engine(step);
				std::string name = textArg(step, "name");
				SoLoud::Queue *queue = new SoLoud::Queue;
				SoLoud::result code = SoLoud::SO_NO_ERROR;
				if (step.get("channels") || step.get("rate"))
					code = queue->setParams(floatArg(step, "rate", 44100.0f), uintArg(step, "channels", false, 1));
				if (step.get("from"))
					queue->setParamsFromAudioSource(soundArg(step, "from"));
				queue->setVolume(floatArg(step, "volume", 1.0f));
				std::map<std::string, SoLoud::Queue *>::iterator old = queues.find(name);
				if (old != queues.end())
					delete old->second;
				queues[name] = queue;
				std::ostringstream line;
				line << "queue " << name << " " << code;
				logLine(line);
			}
			else if (step.op == "queue_add")
			{
				static const char *const keys[] = { "queue", "sound" };
				checkKeys(step, keys, 2);
				SoLoud::Queue &queue = queueArg(step, "queue");
				SoLoud::AudioSource &sound = queuedArg(step, "sound");
				engine(step);
				std::ostringstream line;
				line << "queue_add " << textArg(step, "queue") << " " << queue.play(sound);
				logLine(line);
			}
			else
			{
				static const char *const keys[] = { "queue", "sound" };
				checkKeys(step, keys, 2);
				SoLoud::Queue &queue = queueArg(step, "queue");
				SoLoud::AudioSource *sound = step.get("sound") ? &queuedArg(step, "sound") : 0;
				engine(step);
				std::ostringstream line;
				line << "queue_query " << textArg(step, "queue") << " count=" << queue.getQueueCount();
				if (sound)
					line << " current=" << (queue.isCurrentlyPlaying(*sound) ? 1 : 0);
				logLine(line);
			}
		}

		void busOp(const Step &step)
		{
			if (step.op == "bus")
			{
				static const char *const keys[] = { "name", "channels", "resampler", "visualization", "volume" };
				checkKeys(step, keys, 5);
				engine(step);
				std::string name = textArg(step, "name");
				SoLoud::Bus *bus = new SoLoud::Bus;
				SoLoud::result code = SoLoud::SO_NO_ERROR;
				if (step.get("channels"))
					code = bus->setChannels(uintArg(step, "channels", true, 0));
				if (step.get("resampler"))
					bus->setResampler(uintArg(step, "resampler", true, 0));
				bus->setVisualizationEnable(flagArg(step, "visualization", false));
				bus->setVolume(floatArg(step, "volume", 1.0f));
				std::map<std::string, SoLoud::Bus *>::iterator old = buses.find(name);
				if (old != buses.end())
					delete old->second;
				buses[name] = bus;
				std::ostringstream line;
				line << "bus " << name << " " << code;
				logLine(line);
			}
			else if (step.op == "annex")
			{
				static const char *const keys[] = { "bus", "h" };
				checkKeys(step, keys, 2);
				SoLoud::Bus &bus = busArg(step, "bus");
				SoLoud::handle h = handleArg(step);
				engine(step);
				bus.annexSound(h);
			}
			else if (step.op == "bus_query")
			{
				static const char *const keys[] = { "bus" };
				checkKeys(step, keys, 1);
				SoLoud::Bus &bus = busArg(step, "bus");
				SoLoud::Soloud &s = engine(step);
				std::ostringstream line;
				unsigned int voices = bus.getActiveVoiceCount();
				line << "bus_query " << textArg(step, "bus") << " voices=" << voices << " count=" << s.countAudioSource(bus) << " volumes=";
				for (unsigned int c = 0; c < 10; c++)
					line << (c ? "," : "") << hex32(bus.getApproximateVolume(c));
				logLine(line);
			}
			else if (step.op == "bus_wave")
			{
				static const char *const keys[] = { "bus", "fft" };
				checkKeys(step, keys, 2);
				SoLoud::Bus &bus = busArg(step, "bus");
				bool fft = flagArg(step, "fft", false);
				engine(step);
				std::ostringstream line;
				line << "bus_wave " << textArg(step, "bus");
				if (bus.mInstance)
				{
					float *data = fft ? bus.calcFFT() : bus.getWave();
					samples.insert(samples.end(), data, data + 256);
				}
				else
				{
					line << " none";
				}
				logLine(line);
			}
			else if (step.op == "bus_filter")
			{
				static const char *const keys[] = { "bus", "slot", "filter" };
				checkKeys(step, keys, 3);
				SoLoud::Bus &bus = busArg(step, "bus");
				unsigned int slot = uintArg(step, "slot", true, 0);
				SoLoud::Filter *filter = filterArg(step, "filter");
				engine(step);
				bus.setFilter(slot, filter);
			}
			else
			{
				static const char *const keys[] = { "bus", "channels", "resampler", "visualization" };
				checkKeys(step, keys, 4);
				SoLoud::Bus &bus = busArg(step, "bus");
				engine(step);
				bool logged = false;
				SoLoud::result code = SoLoud::SO_NO_ERROR;
				if (step.get("channels"))
				{
					code = bus.setChannels(uintArg(step, "channels", true, 0));
					logged = true;
				}
				if (step.get("resampler"))
					bus.setResampler(uintArg(step, "resampler", true, 0));
				if (step.get("visualization"))
					bus.setVisualizationEnable(flagArg(step, "visualization", false));
				if (logged)
				{
					std::ostringstream line;
					line << "bus_set " << textArg(step, "bus") << " channels " << code;
					logLine(line);
				}
			}
		}

		void apply(const Step &step)
		{
			if (step.op == "tolerance")
			{
				static const char *const keys[] = { "abs" };
				checkKeys(step, keys, 1);
				if (!step.get("abs"))
					throw ScenarioError(step.line, "`tolerance` needs `abs=`");
			}
			else if (step.op == "init")
			{
				static const char *const keys[] = { "channels", "rate", "buffer", "clip", "left_handed" };
				checkKeys(step, keys, 5);
				unsigned int channels = uintArg(step, "channels", false, 2);
				unsigned int rate = uintArg(step, "rate", false, SoLoud::Soloud::AUTO);
				unsigned int buffer = uintArg(step, "buffer", false, SoLoud::Soloud::AUTO);
				const std::string *clip = step.get("clip");
				unsigned int flags = SoLoud::Soloud::CLIP_ROUNDOFF;
				if (clip && *clip == "hard")
					flags = 0;
				else if (clip && *clip != "roundoff")
					throw ScenarioError(step.line, "unknown clip `" + *clip + "`");
				if (flagArg(step, "left_handed", false))
					flags |= SoLoud::Soloud::LEFT_HANDED_3D;
				// A fresh engine per init, like the Rust runner's new Mixer.
				clearSounds();
				delete soloud;
				soloud = new SoLoud::Soloud;
				SoLoud::result code = soloud->init(flags, SoLoud::Soloud::NULLDRIVER, rate, buffer, channels);
				initialized = code == SoLoud::SO_NO_ERROR;
				std::ostringstream line;
				line << "init " << code;
				logLine(line);
			}
			else if (step.op == "mix")
			{
				static const char *const keys[] = { "frames" };
				checkKeys(step, keys, 1);
				unsigned int frames = uintArg(step, "frames", true, 0);
				if (frames > MAX_MIX_FRAMES)
					throw ScenarioError(step.line, "mix frames must be at most 4096");
				if (!initialized)
					throw ScenarioError(step.line, "mix needs a successful init first");
				size_t start = samples.size();
				samples.resize(start + (size_t)frames * soloud->getBackendChannels());
				soloud->mix(samples.data() + start, frames);
			}
			else if (step.op == "sound")
			{
				static const char *const keys[] = { "name", "channels", "rate", "frames", "pattern", "amp", "period", "seed", "volume", "looping",
					"bits", "loop_point", "auto_stop", "single", "tick", "kill" };
				checkKeys(step, keys, 16);
				engine(step);
				std::string name = textArg(step, "name");
				unsigned int channels = uintArg(step, "channels", false, 1);
				unsigned int frames = uintArg(step, "frames", true, 0);
				if ((unsigned long long)channels * frames > MAX_SOUND_SAMPLES)
					throw ScenarioError(step.line, "channels * frames must be at most 4194304");
				float rate = floatArg(step, "rate", 44100.0f);
				float amp = floatArg(step, "amp", 1.0f);
				std::vector<float> data = renderPattern(step, channels, frames, amp);
				float volume = floatArg(step, "volume", 1.0f);
				bool looping = flagArg(step, "looping", false);
				unsigned int bits = uintArg(step, "bits", false, 32);
				if (bits != 32 && !(amp >= -1.0f && amp <= 1.0f))
					throw ScenarioError(step.line, "`bits` needs `amp` from -1 to 1");
				if (bits != 8 && bits != 16 && bits != 32)
					throw ScenarioError(step.line, "`bits` must be 8, 16 or 32");
				double loopPoint = timeArg(step, "loop_point", 0);
				bool autoStop = flagArg(step, "auto_stop", true);
				bool single = flagArg(step, "single", false);
				bool tick = flagArg(step, "tick", false);
				bool kill = flagArg(step, "kill", false);

				SoLoud::Wav *wav = new SoLoud::Wav;
				SoLoud::result code;
				if (bits == 8)
				{
					std::vector<unsigned char> raw;
					for (size_t i = 0; i < data.size(); i++)
						raw.push_back((unsigned char)(int)(data[i] * 127.0f + 128.0f));
					code = wav->loadRawWave8(raw.data(), (unsigned int)raw.size(), rate, channels);
				}
				else if (bits == 16)
				{
					std::vector<short> raw;
					for (size_t i = 0; i < data.size(); i++)
						raw.push_back((short)(data[i] * 32767.0f));
					code = wav->loadRawWave16(raw.data(), (unsigned int)raw.size(), rate, channels);
				}
				else
				{
					code = wav->loadRawWave(data.data(), (unsigned int)data.size(), rate, channels, true, false);
				}
				if (code == SoLoud::SO_NO_ERROR)
				{
					wav->setLoopPoint(loopPoint);
					wav->setAutoStop(autoStop);
					wav->setSingleInstance(single);
					wav->setInaudibleBehavior(tick, kill);
				}
				storeSound(name, wav, code, volume, looping);
				std::ostringstream line;
				line << "sound " << name << " " << code;
				logLine(line);
			}
			else if (step.op == "load")
			{
				static const char *const keys[] = { "name", "file", "volume", "looping" };
				checkKeys(step, keys, 4);
				engine(step);
				std::string name = textArg(step, "name");
				std::string path = fixtureArg(step);
				float volume = floatArg(step, "volume", 1.0f);
				bool looping = flagArg(step, "looping", false);
				SoLoud::Wav *wav = new SoLoud::Wav;
				SoLoud::result code = wav->load(path.c_str());
				storeSound(name, wav, code, volume, looping);
				std::ostringstream line;
				line << "load " << name << " " << code;
				logLine(line);
			}
			else if (step.op == "stream")
			{
				static const char *const keys[] = { "name", "file", "from", "volume", "looping", "loop_point" };
				checkKeys(step, keys, 6);
				engine(step);
				std::string name = textArg(step, "name");
				std::string path = fixtureArg(step);
				std::string from = step.get("from") ? textArg(step, "from") : "file";
				SoLoud::WavStream *stream = new SoLoud::WavStream;
				SoLoud::result code;
				if (from == "file")
					code = stream->load(path.c_str());
				else if (from == "memory")
					code = stream->loadToMem(path.c_str());
				else if (from == "bytes")
				{
					std::ifstream in(path.c_str(), std::ios::binary);
					std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
					code = stream->loadMem(bytes.data(), (unsigned int)bytes.size(), true, false);
				}
				else
				{
					delete stream;
					throw ScenarioError(step.line, "unknown from `" + from + "`");
				}
				if (code != SoLoud::SO_NO_ERROR)
					delete stream;
				else
				{
					stream->setVolume(floatArg(step, "volume", 1.0f));
					stream->setLooping(flagArg(step, "looping", false));
					stream->setLoopPoint(timeArg(step, "loop_point", 0));
					std::map<std::string, SoLoud::WavStream *>::iterator old = streams.find(name);
					if (old != streams.end())
						delete old->second;
					streams[name] = stream;
				}
				std::ostringstream line;
				line << "stream " << name << " " << code;
				logLine(line);
			}
			else if (step.op == "samples")
			{
				static const char *const keys[] = { "name" };
				checkKeys(step, keys, 1);
				const std::string &name = textArg(step, "name");
				std::map<std::string, SoLoud::Wav *>::iterator it = sounds.find(name);
				if (it == sounds.end())
					throw ScenarioError(step.line, "no sound named `" + name + "`");
				const SoLoud::Wav &wav = *it->second;
				samples.insert(samples.end(), wav.mData, wav.mData + (size_t)wav.mSampleCount * wav.mChannels);
				std::ostringstream line;
				line << "samples " << name << " " << wav.mChannels << " " << wav.mSampleCount << " " << hex32(wav.mBaseSamplerate);
				logLine(line);
			}
			else if (step.op == "play")
			{
				static const char *const keys[] = { "sound", "as", "volume", "pan", "paused", "clock", "background", "bus", "via" };
				checkKeys(step, keys, 9);
				if (step.get("clock") && step.get("paused"))
					throw ScenarioError(step.line, "`clock` and `paused` cannot be combined");
				if (step.get("bus") && step.get("via"))
					throw ScenarioError(step.line, "`bus` and `via` cannot be combined");
				bool background = flagArg(step, "background", false);
				if (step.get("background") && (step.get("pan") || step.get("clock") || step.get("bus") || step.get("via")))
					throw ScenarioError(step.line, "`background` cannot be combined with `pan`, `clock`, `bus` or `via`");
				SoLoud::AudioSource &source = sourceArg(step, "sound");
				SoLoud::Bus *through = step.get("bus") ? &busArg(step, "bus") : 0;
				SoLoud::handle via = step.get("via") ? namedHandle(step, "via") : 0;
				std::string name = textArg(step, "as");
				// Bus::play's volume defaults to 1, Soloud::play's to -1 (the source's).
				float volume = floatArg(step, "volume", through ? 1.0f : -1.0f);
				float pan = floatArg(step, "pan", 0.0f);
				bool paused = flagArg(step, "paused", false);
				SoLoud::Soloud &s = engine(step);
				SoLoud::handle h;
				if (background)
					h = s.playBackground(source, volume, paused);
				else if (through && step.get("clock"))
					h = through->playClocked(timeArg(step, "clock", 0), source, volume, pan);
				else if (through)
					h = through->play(source, volume, pan, paused);
				else if (step.get("clock"))
					h = s.playClocked(timeArg(step, "clock", 0), source, volume, pan, via);
				else
					h = s.play(source, volume, pan, paused, via);
				handles[name] = h;
				std::ostringstream line;
				line << "play " << name << " " << h;
				logLine(line);
			}
			else if (step.op == "stop")
			{
				static const char *const keys[] = { "h" };
				checkKeys(step, keys, 1);
				SoLoud::handle h = handleArg(step);
				engine(step).stop(h);
			}
			else if (step.op == "stop_all")
			{
				checkKeys(step, 0, 0);
				engine(step).stopAll();
			}
			else if (step.op == "set")
			{
				static const char *const keys[] = { "h", "volume", "pan", "pause", "looping", "protect", "delay", "speed",
					"loop_point", "auto_stop", "tick", "kill", "abs_l", "abs_r", "channel", "channel_volume", "rate", "seek" };
				checkKeys(step, keys, 18);
				if ((step.get("tick") != 0) != (step.get("kill") != 0) || (step.get("abs_l") != 0) != (step.get("abs_r") != 0) ||
					(step.get("channel") != 0) != (step.get("channel_volume") != 0))
					throw ScenarioError(step.line, "`tick`/`kill`, `abs_l`/`abs_r` and `channel`/`channel_volume` go in pairs");
				std::string name = textArg(step, "h");
				SoLoud::handle h = handleArg(step);
				float volume = floatArg(step, "volume", 0.0f);
				float pan = floatArg(step, "pan", 0.0f);
				bool pause = flagArg(step, "pause", false);
				bool looping = flagArg(step, "looping", false);
				bool protect = flagArg(step, "protect", false);
				unsigned int delay = uintArg(step, "delay", false, 0);
				float speed = floatArg(step, "speed", 0.0f);
				SoLoud::Soloud &s = engine(step);
				if (step.get("volume"))
					s.setVolume(h, volume);
				if (step.get("pan"))
					s.setPan(h, pan);
				if (step.get("pause"))
					s.setPause(h, pause);
				if (step.get("looping"))
					s.setLooping(h, looping);
				if (step.get("protect"))
					s.setProtectVoice(h, protect);
				if (step.get("delay"))
					s.setDelaySamples(h, delay);
				if (step.get("speed"))
				{
					std::ostringstream line;
					line << "set " << name << " speed " << s.setRelativePlaySpeed(h, speed);
					logLine(line);
				}
				if (step.get("loop_point"))
					s.setLoopPoint(h, timeArg(step, "loop_point", 0));
				if (step.get("auto_stop"))
					s.setAutoStop(h, flagArg(step, "auto_stop", true));
				if (step.get("tick"))
					s.setInaudibleBehavior(h, flagArg(step, "tick", false), flagArg(step, "kill", false));
				if (step.get("abs_l"))
					s.setPanAbsolute(h, floatArg(step, "abs_l", 0.0f), floatArg(step, "abs_r", 0.0f));
				if (step.get("channel"))
					s.setChannelVolume(h, uintArg(step, "channel", true, 0), floatArg(step, "channel_volume", 0.0f));
				if (step.get("rate"))
					s.setSamplerate(h, floatArg(step, "rate", 0.0f));
				if (step.get("seek"))
				{
					std::ostringstream line;
					line << "set " << name << " seek " << s.seek(h, timeArg(step, "seek", 0));
					logLine(line);
				}
			}
			else if (step.op == "global")
			{
				static const char *const keys[] = { "volume", "clip_scale", "resampler", "max_active", "pause_all" };
				checkKeys(step, keys, 5);
				float volume = floatArg(step, "volume", 0.0f);
				float scale = floatArg(step, "clip_scale", 0.0f);
				const std::string *resampler = step.get("resampler");
				unsigned int mode = SoLoud::Soloud::RESAMPLER_LINEAR;
				if (resampler)
				{
					if (*resampler == "point")
						mode = SoLoud::Soloud::RESAMPLER_POINT;
					else if (*resampler == "linear")
						mode = SoLoud::Soloud::RESAMPLER_LINEAR;
					else if (*resampler == "catmullrom")
						mode = SoLoud::Soloud::RESAMPLER_CATMULLROM;
					else
						throw ScenarioError(step.line, "unknown resampler `" + *resampler + "`");
				}
				unsigned int maxActive = uintArg(step, "max_active", false, 0);
				SoLoud::Soloud &s = engine(step);
				if (step.get("volume"))
					s.setGlobalVolume(volume);
				if (step.get("clip_scale"))
					s.setPostClipScaler(scale);
				if (resampler)
					s.setMainResampler(mode);
				if (step.get("max_active"))
				{
					std::ostringstream line;
					line << "global max_active " << s.setMaxActiveVoiceCount(maxActive);
					logLine(line);
				}
				if (step.get("pause_all"))
					s.setPauseAll(flagArg(step, "pause_all", false));
			}
			else if (step.op == "query")
			{
				static const char *const keys[] = { "h" };
				checkKeys(step, keys, 1);
				std::string name = textArg(step, "h");
				SoLoud::handle h = handleArg(step);
				SoLoud::Soloud &s = engine(step);
				std::ostringstream line;
				line << "query " << name
					<< " valid=" << (s.isValidVoiceHandle(h) ? 1 : 0)
					<< " paused=" << (s.getPause(h) ? 1 : 0)
					<< " looping=" << (s.getLooping(h) ? 1 : 0)
					<< " protect=" << (s.getProtectVoice(h) ? 1 : 0)
					<< " loops=" << s.getLoopCount(h)
					<< " volume=" << hex32(s.getVolume(h))
					<< " overall=" << hex32(s.getOverallVolume(h))
					<< " pan=" << hex32(s.getPan(h))
					<< " speed=" << hex32(s.getRelativePlaySpeed(h))
					<< " rate=" << hex32(s.getSamplerate(h))
					<< " time=" << hex64(s.getStreamTime(h))
					<< " pos=" << hex64(s.getStreamPosition(h))
					<< " loop_point=" << hex64(s.getLoopPoint(h))
					<< " auto_stop=" << (s.getAutoStop(h) ? 1 : 0)
					<< " info=" << hex32(s.getInfo(h, 0));
				logLine(line);
			}
			else if (step.op == "mix16")
			{
				static const char *const keys[] = { "frames" };
				checkKeys(step, keys, 1);
				unsigned int frames = uintArg(step, "frames", true, 0);
				if (frames > MAX_MIX_FRAMES)
					throw ScenarioError(step.line, "mix frames must be at most 4096");
				SoLoud::Soloud &s = engine(step);
				std::vector<short> out((size_t)frames * s.getBackendChannels());
				s.mixSigned16(out.data(), frames);
				for (size_t i = 0; i < out.size(); i++)
					samples.push_back((float)out[i]);
			}
			else if (step.op == "sound_query")
			{
				static const char *const keys[] = { "name" };
				checkKeys(step, keys, 1);
				const std::string &name = textArg(step, "name");
				std::map<std::string, SoLoud::WavStream *>::iterator stream = streams.find(name);
				SoLoud::AudioSource *source;
				double length;
				if (!sounds.count(name) && stream != streams.end())
				{
					source = stream->second;
					length = stream->second->getLength();
				}
				else
				{
					SoLoud::Wav &wav = soundArg(step, "name");
					source = &wav;
					length = wav.getLength();
				}
				std::ostringstream line;
				line << "sound_query " << name << " length=" << hex64(length)
					<< " count=" << engine(step).countAudioSource(*source);
				logLine(line);
			}
			else if (step.op == "stop_sound")
			{
				static const char *const keys[] = { "name", "source" };
				checkKeys(step, keys, 2);
				SoLoud::AudioSource &wav = sourceArg(step, "name");
				bool source = flagArg(step, "source", false);
				engine(step);
				if (source)
					wav.stop();
				else
					soloud->stopAudioSource(wav);
			}
			else if (step.op == "group")
			{
				static const char *const keys[] = { "as" };
				checkKeys(step, keys, 1);
				std::string name = textArg(step, "as");
				SoLoud::handle g = engine(step).createVoiceGroup();
				handles[name] = g;
				std::ostringstream line;
				line << "group " << name << " " << g;
				logLine(line);
			}
			else if (step.op == "group_add" || step.op == "group_destroy" || step.op == "group_query")
			{
				static const char *const addKeys[] = { "group", "h" };
				static const char *const groupKeys[] = { "group" };
				if (step.op == "group_add")
					checkKeys(step, addKeys, 2);
				else
					checkKeys(step, groupKeys, 1);
				std::string name = textArg(step, "group");
				SoLoud::handle g = namedHandle(step, "group");
				SoLoud::Soloud &s = engine(step);
				std::ostringstream line;
				line << step.op << " " << name;
				if (step.op == "group_add")
				{
					SoLoud::handle h = handleArg(step);
					line << " " << textArg(step, "h") << " " << s.addVoiceToGroup(g, h);
				}
				else if (step.op == "group_destroy")
				{
					line << " " << s.destroyVoiceGroup(g);
				}
				else
				{
					bool isGroup = s.isVoiceGroup(g);
					line << " is_group=" << (isGroup ? 1 : 0) << " empty=" << (s.isVoiceGroupEmpty(g) ? 1 : 0);
				}
				logLine(line);
			}
			else if (step.op == "version")
			{
				checkKeys(step, 0, 0);
				std::ostringstream line;
				line << "version " << engine(step).getVersion();
				logLine(line);
			}
			else if (step.op == "fade" || step.op == "oscillate")
			{
				bool fade = step.op == "fade";
				static const char *const fadeKeys[] = { "h", "param", "to", "time" };
				static const char *const oscillateKeys[] = { "h", "param", "from", "to", "time" };
				if (fade)
					checkKeys(step, fadeKeys, 4);
				else
					checkKeys(step, oscillateKeys, 5);
				const std::string &param = textArg(step, "param");
				textArg(step, "to");
				textArg(step, "time");
				if (!fade)
					textArg(step, "from");
				float from = floatArg(step, "from", 0.0f);
				float to = floatArg(step, "to", 0.0f);
				double time = timeArg(step, "time", 0);
				SoLoud::Soloud &s = engine(step);
				if (!step.get("h"))
				{
					if (param != "volume")
						throw ScenarioError(step.line, "without `h`, `param` must be `volume` (the global volume)");
					if (fade)
						s.fadeGlobalVolume(to, time);
					else
						s.oscillateGlobalVolume(from, to, time);
				}
				else
				{
					SoLoud::handle h = handleArg(step);
					if (param == "volume")
						fade ? s.fadeVolume(h, to, time) : s.oscillateVolume(h, from, to, time);
					else if (param == "pan")
						fade ? s.fadePan(h, to, time) : s.oscillatePan(h, from, to, time);
					else if (param == "speed")
						fade ? s.fadeRelativePlaySpeed(h, to, time) : s.oscillateRelativePlaySpeed(h, from, to, time);
					else
						throw ScenarioError(step.line, "unknown param `" + param + "`");
				}
			}
			else if (step.op == "schedule")
			{
				static const char *const keys[] = { "h", "pause", "stop" };
				checkKeys(step, keys, 3);
				SoLoud::handle h = handleArg(step);
				if (!step.get("pause") && !step.get("stop"))
					throw ScenarioError(step.line, "`schedule` needs `pause=` or `stop=`");
				SoLoud::Soloud &s = engine(step);
				if (step.get("pause"))
					s.schedulePause(h, timeArg(step, "pause", 0));
				if (step.get("stop"))
					s.scheduleStop(h, timeArg(step, "stop", 0));
			}
			else if (step.op == "state")
			{
				checkKeys(step, 0, 0);
				SoLoud::Soloud &s = engine(step);
				std::ostringstream line;
				line << "state volume=" << hex32(s.getGlobalVolume())
					<< " clip_scale=" << hex32(s.getPostClipScaler())
					<< " resampler=" << s.getMainResampler()
					<< " max_active=" << s.getMaxActiveVoiceCount();
				logLine(line);
			}
			else if (step.op == "counts")
			{
				checkKeys(step, 0, 0);
				SoLoud::Soloud &s = engine(step);
				std::ostringstream line;
				line << "counts voices=" << s.getVoiceCount() << " active=" << s.getActiveVoiceCount();
				logLine(line);
			}
			else if (step.op == "filter")
			{
				createFilter(step);
			}
			else if (step.op == "sound_filter")
			{
				static const char *const keys[] = { "sound", "slot", "filter" };
				checkKeys(step, keys, 3);
				const std::string &name = textArg(step, "sound");
				SoLoud::AudioSource &source = sounds.count(name) ? (SoLoud::AudioSource &)soundArg(step, "sound") : queueArg(step, "sound");
				unsigned int slot = uintArg(step, "slot", true, 0);
				source.setFilter(slot, filterArg(step, "filter"));
			}
			else if (step.op == "global_filter")
			{
				static const char *const keys[] = { "slot", "filter" };
				checkKeys(step, keys, 2);
				unsigned int slot = uintArg(step, "slot", true, 0);
				SoLoud::Filter *filter = filterArg(step, "filter");
				engine(step).setGlobalFilter(slot, filter);
			}
			else if (step.op == "filter_set" || step.op == "filter_fade" || step.op == "filter_oscillate" || step.op == "filter_query")
			{
				filterParameter(step);
			}
			else if (step.op == "filter_info")
			{
				static const char *const keys[] = { "name" };
				checkKeys(step, keys, 1);
				textArg(step, "name");
				SoLoud::Filter &f = *filterArg(step, "name");
				int count = f.getParamCount();
				std::ostringstream line;
				line << "filter_info " << textArg(step, "name") << " count=" << count;
				for (int i = 0; i <= count; i++)
				{
					const char *name = f.getParamName(i);
					line << " " << i << ":" << (name ? name : "-") << ":" << f.getParamType(i) << ":" << hex32(f.getParamMax(i))
						<< ":" << hex32(f.getParamMin(i));
				}
				logLine(line);
			}
			else if (step.op == "fft")
			{
				fftDump(step);
			}
			else if (step.op == "bus" || step.op == "annex" || step.op == "bus_query" || step.op == "bus_wave" || step.op == "bus_filter" || step.op == "bus_set")
			{
				busOp(step);
			}
			else if (step.op == "queue" || step.op == "queue_add" || step.op == "queue_query")
			{
				queueOp(step);
			}
			else if (step.op == "sound3d" || step.op == "play3d" || step.op == "listener" || step.op == "set3d" || step.op == "update3d" ||
				step.op == "speaker" || step.op == "sound_speed")
			{
				spatialOp(step);
			}
			else
			{
				throw ScenarioError(step.line, "unsupported operation `" + step.op + "`");
			}
		}
	};

	std::string stem(const std::string &path)
	{
		size_t slash = path.find_last_of("/\\");
		std::string name = slash == std::string::npos ? path : path.substr(slash + 1);
		size_t dot = name.find_last_of('.');
		return dot == std::string::npos ? name : name.substr(0, dot);
	}

	// <scenario dir>/../fixtures
	std::string fixtureDirFor(const std::string &scenarioPath)
	{
		size_t slash = scenarioPath.find_last_of("/\\");
		std::string dir = slash == std::string::npos ? "." : scenarioPath.substr(0, slash);
		return dir + "/../fixtures";
	}

	bool writeOutputs(const std::string &dir, const std::string &name, const Runner &runner)
	{
		std::ofstream samples((dir + "/" + name + ".f32").c_str(), std::ios::binary);
		for (size_t i = 0; i < runner.samples.size(); i++)
		{
			unsigned char bytes[4];
			unsigned int bits;
			std::memcpy(&bits, &runner.samples[i], 4);
			for (int b = 0; b < 4; b++)
				bytes[b] = (unsigned char)(bits >> (8 * b));
			samples.write((const char *)bytes, 4);
		}
		std::ofstream log((dir + "/" + name + ".log").c_str(), std::ios::binary);
		for (size_t i = 0; i < runner.log.size(); i++)
			log << runner.log[i] << "\n";
		return samples.good() && log.good();
	}
}

int main(int argc, char **argv)
{
	if (argc < 3)
	{
		std::fprintf(stderr, "usage: %s <output dir> <scenario.scn>...\n", argv[0]);
		return 2;
	}
	int failures = 0;
	for (int i = 2; i < argc; i++)
	{
		std::string path = argv[i];
		std::string name = stem(path);
		try
		{
			std::ifstream in(path.c_str(), std::ios::binary);
			if (!in)
				throw ScenarioError(0, "cannot open file");
			std::vector<Step> steps = parse(in);
			Runner *runner = new Runner(fixtureDirFor(path));
			try
			{
				for (size_t s = 0; s < steps.size(); s++)
					runner->apply(steps[s]);
			}
			catch (...)
			{
				delete runner;
				throw;
			}
			bool written = writeOutputs(argv[1], name, *runner);
			delete runner;
			if (!written)
				throw ScenarioError(0, std::string("cannot write output to ") + argv[1]);
			std::printf("%s: ok\n", name.c_str());
		}
		catch (const ScenarioError &error)
		{
			std::fprintf(stderr, "%s:%d: %s\n", path.c_str(), error.line, error.what());
			failures++;
		}
	}
	return failures == 0 ? 0 : 1;
}

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
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "soloud.h"
#include "soloud_wav.h"

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

	struct Runner
	{
		SoLoud::Soloud *soloud;
		bool initialized;
		std::map<std::string, SoLoud::Wav *> sounds;
		std::map<std::string, SoLoud::handle> handles;
		std::vector<float> samples;
		std::vector<std::string> log;
		// tests/fixtures/, which `file=` paths are relative to.
		std::string fixtureDir;

		explicit Runner(const std::string &aFixtureDir) : soloud(0), initialized(false), fixtureDir(aFixtureDir) {}

		~Runner()
		{
			clearSounds();
			delete soloud;
		}

		// A Wav stops its voices in the engine when destroyed, so sounds go
		// before the engine.
		void clearSounds()
		{
			for (std::map<std::string, SoLoud::Wav *>::iterator it = sounds.begin(); it != sounds.end(); ++it)
				delete it->second;
			sounds.clear();
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
				static const char *const keys[] = { "channels", "rate", "buffer", "clip" };
				checkKeys(step, keys, 4);
				unsigned int channels = uintArg(step, "channels", false, 2);
				unsigned int rate = uintArg(step, "rate", false, SoLoud::Soloud::AUTO);
				unsigned int buffer = uintArg(step, "buffer", false, SoLoud::Soloud::AUTO);
				const std::string *clip = step.get("clip");
				unsigned int flags = SoLoud::Soloud::CLIP_ROUNDOFF;
				if (clip && *clip == "hard")
					flags = 0;
				else if (clip && *clip != "roundoff")
					throw ScenarioError(step.line, "unknown clip `" + *clip + "`");
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
				static const char *const keys[] = { "name", "channels", "rate", "frames", "pattern", "amp", "period", "seed", "volume", "looping" };
				checkKeys(step, keys, 10);
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

				SoLoud::Wav *wav = new SoLoud::Wav;
				SoLoud::result code = wav->loadRawWave(data.data(), (unsigned int)data.size(), rate, channels, true, false);
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
				static const char *const keys[] = { "sound", "as", "volume", "pan", "paused", "clock" };
				checkKeys(step, keys, 6);
				const std::string &soundName = textArg(step, "sound");
				std::string name = textArg(step, "as");
				float volume = floatArg(step, "volume", -1.0f);
				float pan = floatArg(step, "pan", 0.0f);
				bool paused = flagArg(step, "paused", false);
				if (step.get("clock") && step.get("paused"))
					throw ScenarioError(step.line, "`clock` and `paused` cannot be combined");
				std::map<std::string, SoLoud::Wav *>::iterator it = sounds.find(soundName);
				if (it == sounds.end())
					throw ScenarioError(step.line, "no sound named `" + soundName + "`");
				SoLoud::handle h = step.get("clock")
					? engine(step).playClocked(timeArg(step, "clock", 0), *it->second, volume, pan)
					: engine(step).play(*it->second, volume, pan, paused);
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
				static const char *const keys[] = { "h", "volume", "pan", "pause", "looping", "protect", "delay", "speed" };
				checkKeys(step, keys, 8);
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
			}
			else if (step.op == "global")
			{
				static const char *const keys[] = { "volume", "clip_scale", "resampler", "max_active" };
				checkKeys(step, keys, 4);
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
					<< " pos=" << hex64(s.getStreamPosition(h));
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

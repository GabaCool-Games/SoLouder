// Render-time benchmark of the C++ reference: builds the cases of
// crates/solouder-bench (same names, same voices, tones, filters and 3D
// motion) on the null driver and times each Soloud::mix call. Prints the
// same tab-separated columns as solouder-bench; scripts/run_bench.py runs
// both and puts them side by side.
//
// Usage: reference_bench [--seconds S] [--case SUBSTRING] [--buffers 128,256]
//
// The sources_* cases play the generated sources (Noise, Vic, Sfxr,
// Monotone, TedSid, Ay, Speech, Vizsn), as solouder-bench's `sources`
// feature does. Streams are decoded inside mix here; the Rust renderer only copies audio
// the stream worker decoded, so the streams_* rows do not compare the same
// work. Build in Release.

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

#include "soloud.h"
#include "soloud_ay.h"
#include "soloud_bassboostfilter.h"
#include "soloud_biquadresonantfilter.h"
#include "soloud_bus.h"
#include "soloud_dcremovalfilter.h"
#include "soloud_echofilter.h"
#include "soloud_fftfilter.h"
#include "soloud_flangerfilter.h"
#include "soloud_freeverbfilter.h"
#include "soloud_lofifilter.h"
#include "soloud_monotone.h"
#include "soloud_noise.h"
#include "soloud_robotizefilter.h"
#include "soloud_sfxr.h"
#include "soloud_speech.h"
#include "soloud_tedsid.h"
#include "soloud_vic.h"
#include "soloud_vizsn.h"
#include "soloud_waveshaperfilter.h"
#include "soloud_wav.h"
#include "soloud_wavstream.h"

namespace
{
	const unsigned int OUTPUT_RATE = 48000;
	const unsigned int MIX_FRAMES = 2048; // smallest buffer size, as solouder-cpal
	const unsigned int TONES = 8;
	const unsigned int TONE_FRAMES = 88200;
	const unsigned int SPATIAL_VOICES = 256;
	const unsigned int STREAM_VOICES = 16;
	const unsigned int SOURCE_VOICES = 16;
	// crates/solouder-bench/src/cases_sources.rs: the texts.
	const char *SPEECH_TEXT = "The quick brown fox jumps over the lazy dog.";
	const char *VIZSN_TEXT = "hyv piv, mit kuuluu";
	typedef std::chrono::steady_clock Clock;

	// crates/solouder-bench/src/signal.rs: tone_samples and voice_options.
	std::vector<float> toneSamples(unsigned int k)
	{
		double frequency = 110.0 * (k + 1.0) * 1.25;
		double step = 6.283185307179586 * frequency / 44100.0;
		std::vector<float> samples(TONE_FRAMES);
		for (unsigned int i = 0; i < TONE_FRAMES; i++)
			samples[i] = (float)(0.5 * std::sin(step * i));
		return samples;
	}

	float voicePan(unsigned int i)
	{
		return (float)((i * 37) % 201) / 100.0f - 1.0f;
	}

	struct Bench
	{
		std::vector<std::unique_ptr<SoLoud::Filter>> filters;
		std::vector<std::unique_ptr<SoLoud::AudioSource>> sources;
		std::vector<SoLoud::handle> spatial;
		unsigned int step = 0;
		SoLoud::Soloud soloud; // destroyed first, stopping every voice

		SoLoud::Wav &tone(unsigned int k)
		{
			SoLoud::Wav *wav = new SoLoud::Wav;
			std::vector<float> samples = toneSamples(k);
			wav->loadRawWave(samples.data(), TONE_FRAMES, 44100.0f, 1, true);
			wav->setLooping(1);
			sources.emplace_back(wav);
			return *wav;
		}

		SoLoud::Bus &bus()
		{
			SoLoud::Bus *bus = new SoLoud::Bus;
			sources.emplace_back(bus);
			return *bus;
		}
	};

	bool init(Bench &b, unsigned int buffer, unsigned int maxActive, unsigned int resampler)
	{
		unsigned int size = buffer > MIX_FRAMES ? buffer : MIX_FRAMES;
		if (b.soloud.init(SoLoud::Soloud::CLIP_ROUNDOFF, SoLoud::Soloud::NULLDRIVER, OUTPUT_RATE, size, 2) != SoLoud::SO_NO_ERROR)
			return false;
		b.soloud.setMaxActiveVoiceCount(maxActive);
		b.soloud.setMainResampler(resampler);
		return true;
	}

	void orbit(unsigned int i, unsigned int t, float *position, float *velocity)
	{
		float radius = 2.0f + (float)(i % 16);
		float angle = (float)i * 0.1f + (float)t * 5.0f / radius * 0.01f;
		position[0] = radius * std::cos(angle);
		position[1] = 0.0f;
		position[2] = radius * std::sin(angle);
		velocity[0] = -5.0f * std::sin(angle);
		velocity[1] = 0.0f;
		velocity[2] = 5.0f * std::cos(angle);
	}

	// The generators of case sources_<kind>, looping (see
	// crates/solouder-bench/src/cases_sources.rs); voice i plays
	// sources[i % size].
	bool buildSources(Bench &b, const std::string &kind, unsigned int buffer)
	{
		if (!init(b, buffer, SOURCE_VOICES, SoLoud::Soloud::RESAMPLER_LINEAR))
			return false;
		std::vector<SoLoud::AudioSource *> sources;
		std::string fixtures = std::string(SOLOUDER_FIXTURE_DIR) + "/";
		if (kind == "noise")
		{
			SoLoud::Noise *noise = new SoLoud::Noise;
			noise->setType(SoLoud::Noise::PINK);
			sources.push_back(noise);
		}
		else if (kind == "vic")
		{
			SoLoud::Vic *vic = new SoLoud::Vic;
			vic->setRegister(SoLoud::Vic::BASS, 200);
			vic->setRegister(SoLoud::Vic::ALTO, 210);
			vic->setRegister(SoLoud::Vic::SOPRANO, 220);
			sources.push_back(vic);
		}
		else if (kind == "sfxr")
		{
			for (int p = 0; p < 7; p++)
			{
				SoLoud::Sfxr *sfxr = new SoLoud::Sfxr;
				sources.push_back(sfxr);
				if (sfxr->loadPreset(p, p + 1) != SoLoud::SO_NO_ERROR)
					return false;
			}
		}
		else if (kind == "monotone")
		{
			SoLoud::Monotone *song = new SoLoud::Monotone;
			sources.push_back(song);
			if (song->load((fixtures + "monotone/demo.mon").c_str()) != SoLoud::SO_NO_ERROR)
				return false;
			song->setParams(4, SoLoud::Soloud::WAVE_SQUARE);
		}
		else if (kind == "tedsid")
		{
			SoLoud::TedSid *tedsid = new SoLoud::TedSid;
			sources.push_back(tedsid);
			if (tedsid->load((fixtures + "chipdump/sidted.zx7.dump").c_str()) != SoLoud::SO_NO_ERROR)
				return false;
		}
		else if (kind == "ay")
		{
			SoLoud::Ay *ay = new SoLoud::Ay;
			sources.push_back(ay);
			if (ay->load((fixtures + "chipdump/ay_ay.dump").c_str()) != SoLoud::SO_NO_ERROR)
				return false;
		}
		else if (kind == "speech")
		{
			SoLoud::Speech *speech = new SoLoud::Speech;
			sources.push_back(speech);
			if (speech->setText(SPEECH_TEXT) != SoLoud::SO_NO_ERROR)
				return false;
		}
		else if (kind == "vizsn")
		{
			SoLoud::Vizsn *vizsn = new SoLoud::Vizsn;
			std::vector<char> text(VIZSN_TEXT, VIZSN_TEXT + std::strlen(VIZSN_TEXT) + 1);
			vizsn->setText(text.data());
			sources.push_back(vizsn);
		}
		else
			return false;
		for (SoLoud::AudioSource *source : sources)
		{
			source->setLooping(1);
			b.sources.emplace_back(source);
		}
		for (unsigned int i = 0; i < SOURCE_VOICES; i++)
			b.soloud.play(*sources[i % sources.size()], 0.05f, voicePan(i));
		return true;
	}

	// Builds case `name` (see crates/solouder-bench/src/cases.rs).
	bool build(Bench &b, const std::string &name, unsigned int buffer)
	{
		static const char *resamplers[3] = {"point", "linear", "catmullrom"};
		static const unsigned int counts[4] = {16, 64, 256, 1023};
		for (unsigned int c = 0; c < 4; c++)
			for (unsigned int r = 0; r < 3; r++)
			{
				if (name != "voices" + std::to_string(counts[c]) + "_" + resamplers[r])
					continue;
				if (!init(b, buffer, counts[c], r))
					return false;
				std::vector<SoLoud::Wav *> tones;
				for (unsigned int k = 0; k < TONES; k++)
					tones.push_back(&b.tone(k));
				for (unsigned int i = 0; i < counts[c]; i++)
					b.soloud.play(*tones[i % TONES], 0.05f, voicePan(i));
				return true;
			}
		if (name == "buses")
		{
			if (!init(b, buffer, 128, SoLoud::Soloud::RESAMPLER_LINEAR))
				return false;
			std::vector<SoLoud::Wav *> tones;
			for (unsigned int k = 0; k < TONES; k++)
				tones.push_back(&b.tone(k));
			SoLoud::Bus &top = b.bus();
			b.soloud.play(top);
			std::vector<SoLoud::Bus *> middle, leaves;
			for (unsigned int m = 0; m < 4; m++)
			{
				middle.push_back(&b.bus());
				top.play(*middle.back());
			}
			for (unsigned int l = 0; l < 16; l++)
			{
				leaves.push_back(&b.bus());
				middle[l / 4]->play(*leaves.back());
			}
			for (unsigned int i = 0; i < 64; i++)
				leaves[i / 4]->play(*tones[i % TONES], 0.05f, voicePan(i));
			return true;
		}
		if (name == "filters")
		{
			if (!init(b, buffer, 64, SoLoud::Soloud::RESAMPLER_LINEAR))
				return false;
			b.filters.emplace_back(new SoLoud::BiquadResonantFilter);
			b.filters.emplace_back(new SoLoud::LofiFilter);
			b.filters.emplace_back(new SoLoud::EchoFilter);
			b.filters.emplace_back(new SoLoud::WaveShaperFilter);
			b.filters.emplace_back(new SoLoud::DCRemovalFilter);
			b.filters.emplace_back(new SoLoud::FFTFilter);
			b.filters.emplace_back(new SoLoud::BassboostFilter);
			b.filters.emplace_back(new SoLoud::RobotizeFilter);
			b.filters.emplace_back(new SoLoud::FlangerFilter);
			b.filters.emplace_back(new SoLoud::FreeverbFilter);
			std::vector<SoLoud::Wav *> sounds;
			for (unsigned int k = 0; k < 10; k++)
			{
				sounds.push_back(&b.tone(k % TONES));
				sounds.back()->setFilter(0, b.filters[k].get());
			}
			b.filters.emplace_back(new SoLoud::BiquadResonantFilter);
			b.soloud.setGlobalFilter(0, b.filters.back().get());
			for (unsigned int i = 0; i < 30; i++)
				b.soloud.play(*sounds[i % 10], 0.05f, voicePan(i));
			return true;
		}
		static const char *streams[4][2] = {
			{"wav", "wav/s16_stereo_44100.wav"},
			{"flac", "flac/s16_stereo_44100.flac"},
			{"ogg", "ogg/vorbis_stereo_44100.ogg"},
			{"mp3", "mp3/mpeg1_stereo_44100_cbr128.mp3"}};
		for (unsigned int s = 0; s < 4; s++)
		{
			if (name != std::string("streams_") + streams[s][0])
				continue;
			if (!init(b, buffer, STREAM_VOICES, SoLoud::Soloud::RESAMPLER_LINEAR))
				return false;
			SoLoud::WavStream *stream = new SoLoud::WavStream;
			b.sources.emplace_back(stream);
			std::string path = std::string(SOLOUDER_FIXTURE_DIR) + "/" + streams[s][1];
			if (stream->loadToMem(path.c_str()) != SoLoud::SO_NO_ERROR)
				return false;
			stream->setLooping(1);
			for (unsigned int i = 0; i < STREAM_VOICES; i++)
				b.soloud.play(*stream, 0.05f, voicePan(i));
			return true;
		}
		if (name.compare(0, 8, "sources_") == 0)
			return buildSources(b, name.substr(8), buffer);
		if (name == "spatial")
		{
			if (!init(b, buffer, SPATIAL_VOICES, SoLoud::Soloud::RESAMPLER_LINEAR))
				return false;
			std::vector<SoLoud::Wav *> tones;
			for (unsigned int k = 0; k < TONES; k++)
				tones.push_back(&b.tone(k));
			for (unsigned int i = 0; i < SPATIAL_VOICES; i++)
			{
				float p[3], v[3];
				orbit(i, 0, p, v);
				b.spatial.push_back(b.soloud.play3d(*tones[i % TONES], p[0], p[1], p[2], v[0], v[1], v[2], 0.05f));
			}
			b.soloud.update3dAudio();
			return true;
		}
		return false;
	}

	// The untimed work before each render: the spatial case's motion.
	void between(Bench &b)
	{
		if (b.spatial.empty())
			return;
		b.step++;
		for (unsigned int i = 0; i < b.spatial.size(); i++)
		{
			float p[3], v[3];
			orbit(i, b.step, p, v);
			b.soloud.set3dSourceParameters(b.spatial[i], p[0], p[1], p[2], v[0], v[1], v[2]);
		}
		float angle = (float)b.step * 0.02f;
		b.soloud.set3dListenerParameters(0, 0, 0, std::sin(angle), 0, -std::cos(angle), 0, 1, 0, 3, 0, 0);
		b.soloud.update3dAudio();
	}

	unsigned int rendersFor(double seconds, unsigned int frames)
	{
		double count = std::ceil(seconds * OUTPUT_RATE / frames);
		return count < 1 ? 1 : (unsigned int)count;
	}

	double micros(Clock::duration d)
	{
		return std::chrono::duration<double, std::micro>(d).count();
	}
}

int main(int argc, char **argv)
{
	double seconds = 2.0;
	std::string filter;
	std::vector<unsigned int> buffers = {128, 256, 480, 512, 1024};
	for (int i = 1; i + 1 < argc; i += 2)
	{
		if (!std::strcmp(argv[i], "--seconds"))
			seconds = std::atof(argv[i + 1]);
		else if (!std::strcmp(argv[i], "--case"))
			filter = argv[i + 1];
		else if (!std::strcmp(argv[i], "--buffers"))
		{
			buffers.clear();
			for (const char *p = argv[i + 1]; *p; p += *p == ',')
				buffers.push_back((unsigned int)std::strtoul(p, (char **)&p, 10));
		}
		else
		{
			std::fprintf(stderr, "unknown argument %s\n", argv[i]);
			return 2;
		}
	}
	const char *names[] = {
		"voices16_point", "voices16_linear", "voices16_catmullrom",
		"voices64_point", "voices64_linear", "voices64_catmullrom",
		"voices256_point", "voices256_linear", "voices256_catmullrom",
		"voices1023_point", "voices1023_linear", "voices1023_catmullrom",
		"buses", "filters", "streams_wav", "streams_flac", "streams_ogg", "streams_mp3", "spatial",
		"sources_noise", "sources_vic", "sources_sfxr", "sources_monotone", "sources_tedsid", "sources_ay",
		"sources_speech", "sources_vizsn"};
	std::printf("case\tbuffer\trenders\tmean_us\tp99_us\tmax_us\tbudget_us\tactive\tnote\n");
	for (const char *name : names)
	{
		if (std::string(name).find(filter) == std::string::npos)
			continue;
		for (unsigned int buffer : buffers)
		{
			std::unique_ptr<Bench> b(new Bench);
			if (!build(*b, name, buffer))
			{
				std::fprintf(stderr, "%s: could not build the case\n", name);
				return 1;
			}
			std::vector<float> output(buffer * 2);
			unsigned int warmup = rendersFor(0.25, buffer);
			if (warmup < 8)
				warmup = 8;
			unsigned int renders = rendersFor(seconds, buffer);
			std::vector<double> times;
			times.reserve(renders);
			for (unsigned int r = 0; r < warmup + renders; r++)
			{
				between(*b);
				Clock::time_point began = Clock::now();
				b->soloud.mix(output.data(), buffer);
				Clock::time_point ended = Clock::now();
				if (r >= warmup)
					times.push_back(micros(ended - began));
			}
			std::vector<double> sorted = times;
			std::sort(sorted.begin(), sorted.end());
			double total = 0;
			for (double t : sorted)
				total += t;
			size_t rank = (sorted.size() * 99 + 99) / 100;
			const char *note = std::strncmp(name, "streams_", 8) ? "" : "decodes in mix";
			std::printf("%s\t%u\t%u\t%.1f\t%.1f\t%.1f\t%.1f\t%u\t%s\n", name, buffer, renders,
				total / sorted.size(), sorted[(rank ? rank : 1) - 1], sorted.back(),
				1e6 * buffer / OUTPUT_RATE, b->soloud.getActiveVoiceCount(), note);
			std::fflush(stdout);
		}
	}
	return 0;
}

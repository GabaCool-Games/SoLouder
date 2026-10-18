// Regression tests for defects fixed in the patched SoLoud C++ reference.
// Each test is run in its own process (see CMakeLists.txt); a crash, hang, or
// nonzero exit fails only that test. Upstream references are in the test names'
// comments and in docs/PORTING.md.

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <new>
#include <string>
#include <thread>
#include <vector>

#include "soloud.h"
#include "soloud_ay.h"
#include "soloud_bassboostfilter.h"
#include "soloud_biquadresonantfilter.h"
#include "soloud_bus.h"
#include "soloud_dcremovalfilter.h"
#include "soloud_echofilter.h"
#include "soloud_fader.h"
#include "soloud_flangerfilter.h"
#include "soloud_freeverbfilter.h"
#include "soloud_lofifilter.h"
#include "soloud_monotone.h"
#include "soloud_noise.h"
#include "soloud_robotizefilter.h"
#include "soloud_sfxr.h"
#include "soloud_waveshaperfilter.h"
#include "soloud_queue.h"
#include "soloud_tedsid.h"
#include "soloud_vic.h"
#include "soloud_wav.h"
#include "soloud_wavstream.h"
// The SID and AY emulators TedSid and Ay play; their state is private.
#define private public
#include "../src/audiosource/tedsid/sid.h"
#include "../src/audiosource/ay/sndchip.h"
#undef private

using namespace SoLoud;

namespace
{
	const unsigned int kRate = 44100;
	int gFailures = 0;

	void check(bool aOk, const char *aWhat)
	{
		if (!aOk)
		{
			std::printf("  FAIL: %s\n", aWhat);
			gFailures++;
		}
	}

	void put32(std::vector<unsigned char> &aOut, uint32_t aValue)
	{
		for (int i = 0; i < 4; i++)
			aOut.push_back((unsigned char)((aValue >> (8 * i)) & 0xff));
	}

	void put16(std::vector<unsigned char> &aOut, uint16_t aValue)
	{
		aOut.push_back((unsigned char)(aValue & 0xff));
		aOut.push_back((unsigned char)(aValue >> 8));
	}

	// 32-bit IEEE float WAV with interleaved samples.
	std::vector<unsigned char> makeWav(unsigned int aChannels, const std::vector<float> &aInterleaved)
	{
		const uint32_t dataBytes = (uint32_t)(aInterleaved.size() * sizeof(float));
		std::vector<unsigned char> out;
		out.insert(out.end(), { 'R', 'I', 'F', 'F' });
		put32(out, 36 + dataBytes);
		out.insert(out.end(), { 'W', 'A', 'V', 'E', 'f', 'm', 't', ' ' });
		put32(out, 16);
		put16(out, 3); // WAVE_FORMAT_IEEE_FLOAT
		put16(out, (uint16_t)aChannels);
		put32(out, kRate);
		put32(out, kRate * aChannels * 4);
		put16(out, (uint16_t)(aChannels * 4));
		put16(out, 32);
		out.insert(out.end(), { 'd', 'a', 't', 'a' });
		put32(out, dataBytes);
		const unsigned char *bytes = (const unsigned char *)aInterleaved.data();
		out.insert(out.end(), bytes, bytes + dataBytes);
		return out;
	}

	std::vector<unsigned char> makeConstantWav(unsigned int aChannels, unsigned int aFrames, float aValue)
	{
		return makeWav(aChannels, std::vector<float>(aFrames * aChannels, aValue));
	}

	result loadWav(Wav &aWav, const std::vector<unsigned char> &aBytes)
	{
		return aWav.loadMem(aBytes.data(), (unsigned int)aBytes.size(), true, false);
	}

	result loadStream(WavStream &aStream, const std::vector<unsigned char> &aBytes)
	{
		return aStream.loadMem(aBytes.data(), (unsigned int)aBytes.size(), true, false);
	}

	bool initNull(Soloud &aSoloud, unsigned int aChannels = 2)
	{
		return aSoloud.init(0, Soloud::NULLDRIVER, kRate, 512, aChannels) == SO_NO_ERROR;
	}

	// Interleaved output, mixed in 512-frame calls.
	std::vector<float> render(Soloud &aSoloud, unsigned int aFrames, unsigned int aChannels = 2)
	{
		std::vector<float> out(aFrames * aChannels, 0.0f);
		for (unsigned int f = 0; f < aFrames; f += 512)
		{
			unsigned int n = aFrames - f < 512 ? aFrames - f : 512;
			aSoloud.mix(&out[f * aChannels], n);
		}
		return out;
	}

	float maxAbsChannel(const std::vector<float> &aOut, unsigned int aChannel, unsigned int aChannels = 2)
	{
		float peak = 0.0f;
		for (size_t i = aChannel; i < aOut.size(); i += aChannels)
			peak = std::fmax(peak, std::fabs(aOut[i]));
		return peak;
	}

	// PR #379: FLAC/MP3/WAV stream reads used aSamplesToRead as the channel
	// stride. The loop-wrap refill reads fewer frames than the buffer stride,
	// so the right channel landed at the wrong offset.
	void wavstream_loop_channel_stride()
	{
		const unsigned int frames = 700; // not a multiple of the 512-frame block
		std::vector<float> pcm(frames * 2);
		for (unsigned int i = 0; i < frames; i++)
		{
			float left = 0.1f + 0.8f * (float)i / (float)frames;
			pcm[i * 2 + 0] = left;
			pcm[i * 2 + 1] = -left;
		}
		WavStream stream;
		check(loadStream(stream, makeWav(2, pcm)) == SO_NO_ERROR, "stream loads");
		stream.setLooping(true);

		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle h = soloud.play(stream);
		std::vector<float> out = render(soloud, 4096);
		check(soloud.getLoopCount(h) > 0, "stream looped at least once");

		unsigned int mismatches = 0;
		for (size_t i = 0; i + 1 < out.size(); i += 2)
		{
			if (std::fabs(out[i] + out[i + 1]) > 1e-4f)
				mismatches++;
		}
		if (mismatches)
			std::printf("  %u frames where right != -left\n", mismatches);
		check(mismatches == 0, "right channel mirrors left across loop wraps");
	}

	// PR #379: WavStreamInstance::hasEnded ignored LOOPING (WavInstance has the guard).
	void wavstream_looping_not_ended()
	{
		WavStream stream;
		check(loadStream(stream, makeConstantWav(2, 700, 0.25f)) == SO_NO_ERROR, "stream loads");
		stream.setLooping(true);
		AudioSourceInstance *instance = stream.createInstance();
		instance->init(stream, 0);
		std::vector<float> buffer(1024 * 2);
		instance->getAudio(buffer.data(), 512, 1024);
		instance->getAudio(buffer.data(), 512, 1024);
		check(!instance->hasEnded(), "looping stream does not report ended");
		delete instance;
	}

	// A WAV stream whose data chunk ends before its declared size reads fewer
	// frames than its length, so its read position never reached the length
	// and the voice played silence forever instead of ending.
	void wavstream_truncated_data_ends()
	{
		std::vector<unsigned char> bytes = makeConstantWav(1, 44100, 0.25f);
		bytes.resize(44 + 1000 * 4);
		WavStream stream;
		check(loadStream(stream, bytes) == SO_NO_ERROR, "stream loads");
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle h = soloud.play(stream);
		std::vector<float> out = render(soloud, 4096);
		check(maxAbsChannel(std::vector<float>(out.begin(), out.begin() + 2 * 900), 0) > 0.1f, "the frames present play");
		check(!soloud.isValidVoiceHandle(h), "the voice ends after the frames present");
	}

	// A WAV stream loaded from a file that is gone when it is played: the
	// instance read through a DiskFile that failed to open.
	void wavstream_missing_file_ends()
	{
		const char *path = "wavstream_missing_file_ends.wav";
		std::vector<unsigned char> bytes = makeConstantWav(1, 4410, 0.25f);
		FILE *f = std::fopen(path, "wb");
		check(f != NULL, "temporary file opens");
		if (!f)
			return;
		std::fwrite(bytes.data(), 1, bytes.size(), f);
		std::fclose(f);
		WavStream stream;
		check(stream.load(path) == SO_NO_ERROR, "stream loads");
		std::remove(path);
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle h = soloud.play(stream);
		std::vector<float> out = render(soloud, 2048);
		check(maxAbsChannel(out, 0) == 0, "nothing plays");
		check(!soloud.isValidVoiceHandle(h), "the voice ends");

		// Looping must not keep the voice alive playing silence.
		stream.setLooping(true);
		handle looping = soloud.play(stream);
		out = render(soloud, 2048);
		check(maxAbsChannel(out, 0) == 0, "nothing plays when looping");
		check(!soloud.isValidVoiceHandle(looping), "the looping voice ends");

		// Also when the voice cannot advance (a step of a whole block or more).
		handle stuck = soloud.play(stream);
		soloud.setRelativePlaySpeed(stuck, 600);
		render(soloud, 2048);
		check(!soloud.isValidVoiceHandle(stuck), "the looping voice that cannot advance ends");
	}

	// #279/#205/#327: inaudible-tick voices filling the active cap made
	// calcActiveVoices_internal return before assigning resample buffers.
	void inaudible_tick_over_active_cap()
	{
		Wav ticking;
		check(loadWav(ticking, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		ticking.setLooping(true);
		ticking.setInaudibleBehavior(true, false);
		Wav normal;
		check(loadWav(normal, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");

		Soloud soloud;
		check(initNull(soloud), "null driver init");
		check(soloud.setMaxActiveVoiceCount(2) == SO_NO_ERROR, "set active cap");
		for (int i = 0; i < 3; i++)
			soloud.play(ticking);
		soloud.play(normal);
		std::vector<float> out = render(soloud, 4096);
		bool finite = true;
		for (float v : out)
			finite = finite && std::isfinite(v);
		check(finite, "output is finite");
	}

	// #298: PROTECTED was ignored when ranking active voices; docs say
	// "Protected voices are always played."
	void protected_voice_stays_active()
	{
		Wav dc;
		check(loadWav(dc, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		dc.setLooping(true);

		Soloud soloud;
		check(initNull(soloud), "null driver init");
		check(soloud.setMaxActiveVoiceCount(2) == SO_NO_ERROR, "set active cap");
		handle quiet = soloud.play(dc, 0.2f, -1.0f); // hard left, quietest voice
		soloud.setProtectVoice(quiet, true);
		for (int i = 0; i < 3; i++)
			soloud.play(dc, 1.0f, 1.0f); // hard right, louder
		std::vector<float> out = render(soloud, 4096);
		check(maxAbsChannel(out, 0) > 0.01f, "protected voice is audible on the left");
	}

	// Patch 0023: setProtectVoice did not mark the active set dirty, so a newly
	// protected voice stayed silent until something else recomputed it.
	void protecting_a_voice_makes_it_active()
	{
		Wav dc;
		check(loadWav(dc, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		dc.setLooping(true);

		Soloud soloud;
		check(initNull(soloud), "null driver init");
		check(soloud.setMaxActiveVoiceCount(1) == SO_NO_ERROR, "set active cap");
		handle quiet = soloud.play(dc, 0.2f, -1.0f); // hard left, quietest voice
		soloud.play(dc, 1.0f, 1.0f);                  // hard right, active
		std::vector<float> out = render(soloud, 1024);
		check(maxAbsChannel(out, 0) < 0.01f, "only the loud voice is mixed at first");
		soloud.setProtectVoice(quiet, true);
		out = render(soloud, 1024);
		check(maxAbsChannel(out, 0) > 0.01f, "the protected voice is mixed in the next render");
	}

	// Patch 0026: setInaudibleBehavior did not mark the active set dirty, so a
	// voice made to tick stayed out of the active set like in patch 0023.
	void ticking_a_voice_makes_it_active()
	{
		Wav dc;
		check(loadWav(dc, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		dc.setLooping(true);

		Soloud soloud;
		check(initNull(soloud), "null driver init");
		check(soloud.setMaxActiveVoiceCount(1) == SO_NO_ERROR, "set active cap");
		handle quiet = soloud.play(dc, 0.2f, -1.0f); // hard left, quietest voice
		soloud.play(dc, 1.0f, 1.0f);                  // hard right, active
		std::vector<float> out = render(soloud, 1024);
		check(maxAbsChannel(out, 0) < 0.01f, "only the loud voice is mixed at first");
		soloud.setInaudibleBehavior(quiet, true, false);
		out = render(soloud, 1024);
		check(maxAbsChannel(out, 0) > 0.01f, "the ticking voice is mixed in the next render");
	}

	// Frames of the left channel that are not silent.
	unsigned int audibleFrames(const std::vector<float> &aOut)
	{
		unsigned int count = 0;
		for (size_t i = 0; i < aOut.size(); i += 2)
		{
			if (aOut[i] != 0.0f)
				count++;
		}
		return count;
	}

	// Patch 0024: a voice was stopped at the end of the mix call in which its
	// last block was read, so when a call ended partway through that block (any
	// device buffer not aligned to 512 frames, or any resampling) the rest of
	// the sound was dropped with a click.
	void sound_tail_survives_unaligned_mix_calls()
	{
		Wav dc;
		check(loadWav(dc, makeConstantWav(1, 1000, 0.5f)) == SO_NO_ERROR, "wav loads");
		const unsigned int resamplers[] = { Soloud::RESAMPLER_POINT, Soloud::RESAMPLER_LINEAR, Soloud::RESAMPLER_CATMULLROM };
		const unsigned int chunks[] = { 256, 441, 480 };
		const float speeds[] = { 1.0f, 0.73f };
		for (unsigned int resampler : resamplers)
		{
			for (float speed : speeds)
			{
				Soloud whole;
				check(initNull(whole), "null driver init");
				whole.setMainResampler(resampler);
				whole.setRelativePlaySpeed(whole.play(dc), speed);
				std::vector<float> reference(4096 * 2, 0.0f);
				whole.mix(reference.data(), 4096);
				unsigned int expected = audibleFrames(reference);

				for (unsigned int chunk : chunks)
				{
					Soloud soloud;
					check(initNull(soloud), "null driver init");
					soloud.setMainResampler(resampler);
					handle h = soloud.play(dc);
					soloud.setRelativePlaySpeed(h, speed);
					std::vector<float> out(4096 * 2, 0.0f);
					for (unsigned int f = 0; f + chunk <= 4096; f += chunk)
						soloud.mix(&out[f * 2], chunk);
					unsigned int played = audibleFrames(out);
					if (played != expected)
						std::printf("  resampler %u, speed %g, %u-frame calls: %u of %u frames\n", resampler, speed, chunk, played, expected);
					check(played == expected, "the whole sound plays");
					check(!soloud.isValidVoiceHandle(h), "the voice stops after the sound");
				}
			}
		}
	}

	// A voice whose fixed-point step rounds to zero skipped resampling but was
	// still panned, mixing the previous voice's scratch data into the output.
	void zero_step_voice_no_stale_scratch()
	{
		Wav loud;
		check(loadWav(loud, makeConstantWav(1, 4410, 0.5f)) == SO_NO_ERROR, "wav loads");
		loud.setLooping(true);
		Wav silent;
		check(loadWav(silent, makeConstantWav(1, 4410, 0.0f)) == SO_NO_ERROR, "wav loads");
		silent.setLooping(true);

		Soloud soloud;
		check(initNull(soloud), "null driver init");
		soloud.play(loud, 1.0f, -1.0f);
		handle slow = soloud.play(silent, 1.0f, 1.0f);
		check(soloud.setRelativePlaySpeed(slow, 1e-7f) == SO_NO_ERROR, "set tiny speed");
		std::vector<float> out = render(soloud, 4096);
		check(maxAbsChannel(out, 1) < 1e-6f, "silent near-stopped voice adds nothing on the right");
	}

	// #343: with a step of 512 source frames or more per output frame, no
	// output frame fits in a 512-frame block, so the resampling loop loaded
	// block after block without ever advancing and mix never returned.
	void extreme_speed_makes_progress()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(1, 4410, 0.5f)) == SO_NO_ERROR, "wav loads");
		wav.setLooping(true);
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle h = soloud.play(wav);
		check(soloud.setRelativePlaySpeed(h, 600.0f) == SO_NO_ERROR, "set speed 600");
		std::vector<float> out = render(soloud, 1024);
		std::printf("  mix returned, peak %g\n", maxAbsChannel(out, 0));
		check(soloud.isValidVoiceHandle(h), "voice is still playing");
	}

	// #392: generic seek looped forever when the scratch held less than one frame.
	void generic_seek_small_scratch()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(2, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		AudioSourceInstance *instance = wav.createInstance();
		instance->init(wav, 0);
		float scratch[4] = {};
		check(instance->seek(0.05, scratch, 1) != SO_NO_ERROR, "seek with sub-frame scratch fails");
		check(instance->seek(0.05, scratch, 0) != SO_NO_ERROR, "seek with empty scratch fails");
		delete instance;
	}

	// Patch 0027: setSamplerate accepts any rate, and a negative one made the
	// fixed-point step a huge unsigned value, so mixBus_internal never wrote a
	// frame and mix hung.
	void negative_sample_rate_renders_silence()
	{
		Wav dc;
		check(loadWav(dc, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle slow = soloud.play(dc, 1.0f, -1.0f);
		handle back = soloud.play(dc, 1.0f, -1.0f);
		soloud.setSamplerate(slow, -44100.0f);
		soloud.setSamplerate(back, -INFINITY);
		std::vector<float> out = render(soloud, 1024);
		check(maxAbsChannel(out, 0) == 0.0f, "the voices render silence");
		check(soloud.isValidVoiceHandle(slow) && soloud.isValidVoiceHandle(back), "the voices keep playing");
	}

	// Patch 0028: seek turned seconds into frames at the voice's playback
	// rate, which includes the relative play speed, but mStreamPosition counts
	// seconds of source; at speed 2 a seek went twice as far.
	void seek_ignores_play_speed()
	{
		std::vector<float> ramp(kRate);
		for (unsigned int i = 0; i < kRate; i++)
			ramp[i] = (float)i / kRate;
		Wav wav;
		check(loadWav(wav, makeWav(1, ramp)) == SO_NO_ERROR, "wav loads");
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle h = soloud.play(wav, 1.0f, 0.0f, true);
		soloud.setRelativePlaySpeed(h, 2.0f);
		check(soloud.seek(h, 0.25) == SO_NO_ERROR, "seek succeeds");
		check(soloud.getStreamPosition(h) == 0.25, "the stream position is the requested time");
		AudioSourceInstance *instance = wav.createInstance();
		instance->init(wav, 0);
		instance->mSamplerate = instance->mBaseSamplerate * 2;
		std::vector<float> scratch(4096);
		check(instance->seek(0.25, scratch.data(), 4096) == SO_NO_ERROR, "instance seek succeeds");
		float sample = -1.0f;
		instance->getAudio(&sample, 1, 1);
		check(sample == ramp[kRate / 4], "the seek lands a quarter second into the source");
		delete instance;
	}

	// Patch 0029: the loop wrap seeks with the mixer scratch, which still holds
	// the frames resampled earlier in the same mix, so any loop point above 0
	// overwrote them with discarded source audio.
	void loop_point_keeps_earlier_frames()
	{
		std::vector<float> saw(2000);
		for (unsigned int i = 0; i < saw.size(); i++)
			saw[i] = (float)(i % 97) / 97.0f - 0.5f;
		std::vector<float> outputs[2];
		for (int run = 0; run < 2; run++)
		{
			Wav wav;
			check(loadWav(wav, makeWav(1, saw)) == SO_NO_ERROR, "wav loads");
			wav.setLooping(true);
			wav.setLoopPoint(run ? 0.02 : 0.0);
			Soloud soloud;
			check(initNull(soloud), "null driver init");
			soloud.play(wav);
			outputs[run] = render(soloud, 1500);
			std::vector<float> more = render(soloud, 400);
			outputs[run].insert(outputs[run].end(), more.begin(), more.end());
		}
		check(outputs[0] == outputs[1], "the frames before the first wrap do not depend on the loop point");
	}

	// Patch 0030: soloud.h's fallback M_PI (used where <math.h> has none, as
	// with MSVC) had 12 digits, so a hard-panned voice leaked about 1e-12 into
	// the other side on Windows but not on Linux.
	void hard_pan_is_silent_on_the_far_side()
	{
		Wav dc;
		check(loadWav(dc, makeConstantWav(1, 4410, 1.0f)) == SO_NO_ERROR, "wav loads");
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		soloud.setPostClipScaler(1.0f);
		soloud.play(dc, 1.0f, 1.0f);
		std::vector<float> out = render(soloud, 512);
		check(maxAbsChannel(out, 0) < 1e-15f, "the left side is silent");
	}

	bool allFinite(const std::vector<float> &aSamples)
	{
		for (float v : aSamples)
			if (!std::isfinite(v))
				return false;
		return true;
	}

	// Patch 0031: filters run at the voice's rate, which falls with its speed.
	// A biquad above that rate's Nyquist frequency had poles outside the unit
	// circle and its output grew to infinity and NaN.
	void biquad_above_voice_nyquist_stays_finite()
	{
		const int types[3] = { BiquadResonantFilter::LOWPASS, BiquadResonantFilter::HIGHPASS, BiquadResonantFilter::BANDPASS };
		for (int type : types)
		{
			BiquadResonantFilter biquad;
			biquad.setParams(type, 5000, 2);
			FilterInstance *instance = biquad.createInstance();
			unsigned int x = 1;
			float peak = 0;
			for (int block = 0; block < 8; block++)
			{
				std::vector<float> buffer(512);
				for (float &v : buffer)
				{
					x ^= x << 13;
					x ^= x >> 17;
					x ^= x << 5;
					v = (float)(x >> 8) / 8388608.0f - 1.0f;
				}
				// A voice at a fifth of 44.1 kHz filters at 8820 Hz.
				instance->filter(buffer.data(), 512, 512, 1, 8820, 0);
				for (float v : buffer)
					if (!(std::fabs(v) <= peak))
						peak = std::isfinite(v) ? std::fabs(v) : INFINITY;
			}
			delete instance;
			check(peak < 10.0f, "biquad output stays bounded");
		}
	}

	// Patch 0032: the lofi filter kept state for 2 channels but filtered up
	// to 8, writing past the instance for channels 2 to 7.
	void lofi_filters_eight_channels()
	{
		LofiFilter lofi;
		lofi.setParams(8000, 4);
		std::vector<float> buffer(8 * 512);
		for (unsigned int c = 0; c < 8; c++)
			for (unsigned int i = 0; i < 512; i++)
				buffer[c * 512 + i] = (float)(i % 50) / 50.0f - 0.5f;
		// Without the patch channels 2 to 7 use whatever follows the
		// instance; fill it so that stale state shows.
		std::vector<unsigned char> memory(sizeof(LofiFilterInstance) + 64 * sizeof(float));
		memset(memory.data(), 0x4f, memory.size());
		LofiFilterInstance *instance = new (memory.data()) LofiFilterInstance(&lofi);
		for (int pass = 0; pass < 4; pass++)
			instance->filter(buffer.data(), 512, 512, 8, 44100, 0);
		instance->~LofiFilterInstance();
		bool same = true;
		for (unsigned int c = 1; c < 8; c++)
			for (unsigned int i = 0; i < 512; i++)
				same = same && buffer[c * 512 + i] == buffer[i];
		check(same, "every channel with the same input gives the same output");
	}

	// Patch 0033: an echo delay set to 0 through setFilterParameter (or faded
	// there) made the delay line 0 frames long and divided by zero.
	void echo_with_zero_delay_survives()
	{
		EchoFilter echo;
		echo.setParams(0.01f, 0.5f);
		FilterInstance *fresh = echo.createInstance();
		fresh->setFilterParameter(EchoFilter::DELAY, 0);
		std::vector<float> buffer(2 * 512, 0.25f);
		fresh->filter(buffer.data(), 512, 512, 2, 44100, 0);
		check(allFinite(buffer), "a zero delay from the start gives finite output");
		delete fresh;
		FilterInstance *running = echo.createInstance();
		running->filter(buffer.data(), 512, 512, 2, 44100, 0);
		running->setFilterParameter(EchoFilter::DELAY, 0);
		running->filter(buffer.data(), 512, 512, 2, 44100, 0);
		check(allFinite(buffer), "a delay set to 0 while running gives finite output");
		delete running;
	}

	// Patch 0034: the waveshaper's documented minimum amount of -1 computed
	// 0 / 0 for samples of magnitude 1.
	void waveshaper_minimum_amount_is_finite()
	{
		WaveShaperFilter shaper;
		check(shaper.setParams(-1) == SO_NO_ERROR, "amount -1 is accepted");
		FilterInstance *instance = shaper.createInstance();
		std::vector<float> buffer(512);
		for (unsigned int i = 0; i < 512; i++)
			buffer[i] = (float)i / 256.0f - 1.0f;
		instance->filter(buffer.data(), 512, 512, 1, 44100, 0);
		check(allFinite(buffer), "output is finite");
		delete instance;
	}

	// Patch 0035: the DC removal instance read its length from its parent
	// filter at the first mix, so a filter destroyed after play was read
	// after it was freed.
	void dc_removal_outlives_its_filter()
	{
		Wav dc;
		check(loadWav(dc, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		void *memory = std::malloc(sizeof(DCRemovalFilter));
		DCRemovalFilter *filter = new (memory) DCRemovalFilter;
		filter->setParams(0.01f);
		dc.setFilter(0, filter);
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		soloud.play(dc);
		dc.setFilter(0, 0);
		filter->~DCRemovalFilter();
		std::memset(memory, 0xdd, sizeof(DCRemovalFilter));
		std::vector<float> out = render(soloud, 1024);
		std::free(memory);

		DCRemovalFilter alive;
		alive.setParams(0.01f);
		dc.setFilter(0, &alive);
		Soloud other;
		check(initNull(other), "null driver init");
		other.play(dc);
		std::vector<float> expected = render(other, 1024);
		check(out == expected, "the output matches a filter that is still alive");
	}

	// Patch 0036: a DC removal length too long for an int made the window
	// INT_MIN frames long, and channel 1 was written at a negative offset.
	void dc_removal_long_length_is_safe()
	{
		DCRemovalFilter dc;
		check(dc.setParams(1e6f) == SO_NO_ERROR, "a long length is accepted");
		FilterInstance *instance = dc.createInstance();
		std::vector<float> buffer(2 * 512, 0.25f);
		instance->filter(buffer.data(), 512, 512, 2, 44100, 0);
		check(allFinite(buffer), "output is finite");
		delete instance;
	}

	// Patch 0037: every filter's setParams accepted NaN, which then filtered
	// the whole voice to NaN.
	void filter_params_reject_nan()
	{
		const float nan = std::numeric_limits<float>::quiet_NaN();
		BiquadResonantFilter biquad;
		check(biquad.setParams(0, nan, 2) == INVALID_PARAMETER, "biquad frequency");
		check(biquad.setParams(0, 1000, nan) == INVALID_PARAMETER, "biquad resonance");
		LofiFilter lofi;
		check(lofi.setParams(nan, 4) == INVALID_PARAMETER, "lofi rate");
		check(lofi.setParams(8000, nan) == INVALID_PARAMETER, "lofi bit depth");
		EchoFilter echo;
		check(echo.setParams(nan) == INVALID_PARAMETER, "echo delay");
		check(echo.setParams(0.1f, nan) == INVALID_PARAMETER, "echo decay");
		check(echo.setParams(0.1f, 0.5f, nan) == INVALID_PARAMETER, "echo filter");
		WaveShaperFilter shaper;
		check(shaper.setParams(nan) == INVALID_PARAMETER, "waveshaper amount");
		DCRemovalFilter dc;
		check(dc.setParams(nan) == INVALID_PARAMETER, "dc removal length");
	}

	// Patch 0038: for a one-sample buffer (a global filter in a one-frame
	// mix) the biquad copied the sample before the buffer into it.
	void biquad_single_sample_stays_in_bounds()
	{
		BiquadResonantFilter biquad;
		FilterInstance *instance = biquad.createInstance();
		float memory[2] = { 123.0f, 0.5f };
		instance->filter(memory + 1, 1, 1, 1, 44100, 0);
		check(memory[1] == 0.5f, "the sample is left dry");
		delete instance;
	}

	std::vector<float> saw(unsigned int aChannels, unsigned int aFrames, unsigned int aPeriod)
	{
		std::vector<float> samples(aChannels * aFrames);
		for (unsigned int c = 0; c < aChannels; c++)
			for (unsigned int i = 0; i < aFrames; i++)
				samples[c * aFrames + i] = (float)(i % aPeriod) / (float)aPeriod - 0.5f;
		return samples;
	}

	// Filters a block of `aChannels` x 512 frames four times over.
	std::vector<float> runFilter(Filter &aFilter, std::vector<float> aBuffer, unsigned int aChannels, float aRate, double aTime)
	{
		FilterInstance *instance = aFilter.createInstance();
		for (int pass = 0; pass < 4; pass++)
			instance->filter(aBuffer.data(), 512, 512, aChannels, aRate, aTime);
		delete instance;
		return aBuffer;
	}

	// Patch 0039: freeverb asserted stereo (a breakpoint in MSVC release
	// builds) and otherwise read a mono voice's right channel from the
	// memory after it.
	void freeverb_leaves_mono_unchanged()
	{
		FreeverbFilter reverb;
		std::vector<float> buffer = saw(2, 512, 40);
		std::vector<float> out = runFilter(reverb, buffer, 1, 44100, 0);
		check(std::equal(buffer.begin(), buffer.begin() + 512, out.begin()), "mono audio is unchanged");
	}

	// Patch 0040: a freeverb instance applied its parameters only once one was
	// changed, so setParams had no effect.
	void freeverb_uses_its_settings()
	{
		FreeverbFilter standard;
		FreeverbFilter small;
		check(small.setParams(0, 0.1f, 0.9f, 0.2f) == SO_NO_ERROR, "settings accepted");
		std::vector<float> impulse(2 * 512, 0.0f);
		impulse[0] = impulse[512] = 1.0f;
		check(runFilter(standard, impulse, 2, 44100, 0) != runFilter(small, impulse, 2, 44100, 0), "the settings change the output");
	}

	// Patch 0041: freeverb and robotize never updated their parameters, so
	// fades and oscillations did nothing.
	void freeverb_and_robotize_fade_parameters()
	{
		FreeverbFilter reverb;
		RobotizeFilter robot;
		Filter *filters[2] = { &reverb, &robot };
		for (Filter *filter : filters)
		{
			FilterInstance *instance = filter->createInstance();
			instance->fadeFilterParameter(0, 0.0f, 1.0, 0);
			std::vector<float> buffer = saw(2, 512, 40);
			instance->filter(buffer.data(), 512, 512, 2, 44100, 2.0);
			check(instance->getFilterParameter(0) == 0.0f, "the wet mix faded to 0");
			delete instance;
		}
	}

	// Patch 0042: robotize divided by a period of 0 frames when its frequency
	// was above the rate (a slow voice), and its phase jumped once the stream
	// time in samples passed the int range.
	void robotize_period_and_phase_are_safe()
	{
		RobotizeFilter robot;
		std::vector<float> slow = runFilter(robot, saw(1, 512, 40), 1, 20, 0);
		check(allFinite(slow), "a rate below the frequency gives finite output");
		// 1e6 s at 44.1 kHz is a whole number of 1470-frame periods.
		check(runFilter(robot, saw(1, 512, 40), 1, 44100, 1e6) == runFilter(robot, saw(1, 512, 40), 1, 44100, 0),
			"the phase after 1e6 s matches the phase at 0");
	}

	// Patch 0043: the flanger advanced its LFO for every sample of every
	// channel, so in stereo it ran at twice its frequency with the channels
	// out of phase.
	void flanger_channels_share_the_lfo()
	{
		FlangerFilter flanger;
		std::vector<float> out = runFilter(flanger, saw(2, 512, 40), 2, 44100, 0);
		check(std::equal(out.begin(), out.begin() + 512, out.begin() + 512), "identical channels stay identical");
	}

	// Patch 0044: a flanger delay of 0 left the delay line empty (a division
	// by zero), and one beyond the int range sized it from INT_MIN.
	void flanger_delay_extremes_are_safe()
	{
		FlangerFilter flanger;
		float delays[2] = { 0.0f, 1e6f };
		for (float delay : delays)
		{
			FilterInstance *instance = flanger.createInstance();
			instance->setFilterParameter(FlangerFilter::DELAY, delay);
			std::vector<float> buffer = saw(1, 512, 40);
			instance->filter(buffer.data(), 512, 512, 1, 44100, 0);
			check(allFinite(buffer), "output is finite");
			delete instance;
		}
	}

	// Patch 0045: as patch 0037, for bassboost, flanger and freeverb.
	void more_filter_params_reject_nan()
	{
		const float nan = std::numeric_limits<float>::quiet_NaN();
		BassboostFilter bass;
		check(bass.setParams(nan) == INVALID_PARAMETER, "bassboost boost");
		FlangerFilter flanger;
		check(flanger.setParams(nan, 10) == INVALID_PARAMETER, "flanger delay");
		check(flanger.setParams(0.005f, nan) == INVALID_PARAMETER, "flanger frequency");
		FreeverbFilter reverb;
		check(reverb.setParams(nan, 0.5f, 0.5f, 1) == INVALID_PARAMETER, "freeverb freeze");
		check(reverb.setParams(0, nan, 0.5f, 1) == INVALID_PARAMETER, "freeverb room size");
		check(reverb.setParams(0, 0.5f, nan, 1) == INVALID_PARAMETER, "freeverb damp");
		check(reverb.setParams(0, 0.5f, 0.5f, nan) == INVALID_PARAMETER, "freeverb width");
	}

	// #345: before init, the active-voice count was 16 but the resample
	// owner array was null, so stopping a voice dereferenced it.
	void play_stop_without_init()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		Soloud soloud;
		handle h = soloud.play(wav);
		soloud.stop(h);
		soloud.stopAll();
	}

	// #404: same state after a failed init (backend not compiled in).
	void play_after_failed_init()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		Soloud soloud;
		check(soloud.init(0, Soloud::SDL2) != SO_NO_ERROR, "init with missing backend fails");
		handle h = soloud.play(wav);
		soloud.stop(h);
		soloud.deinit();
	}

	// #401: decoders' channel counts were trusted; more than MAX_CHANNELS
	// overflowed the fixed-size decode buffer.
	void wav_rejects_too_many_channels()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(16, 2048, 0.25f)) != SO_NO_ERROR, "16-channel wav is rejected");
	}

	void wavstream_rejects_too_many_channels()
	{
		WavStream stream;
		result r = loadStream(stream, makeConstantWav(16, 2048, 0.25f));
		check(r != SO_NO_ERROR, "16-channel stream is rejected");
		if (r == SO_NO_ERROR)
		{
			Soloud soloud;
			check(initNull(soloud), "null driver init");
			soloud.play(stream);
			render(soloud, 2048);
		}
	}

	// mapResampleBuffers_internal used char live[256] with an assert, while
	// setMaxActiveVoiceCount accepts up to VOICE_COUNT - 1.
	void active_voice_cap_above_255()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(1, 4410, 0.001f)) == SO_NO_ERROR, "wav loads");
		wav.setLooping(true);
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		check(soloud.setMaxActiveVoiceCount(300) == SO_NO_ERROR, "set active cap to 300");
		for (int i = 0; i < 300; i++)
			soloud.play(wav);
		render(soloud, 1024);
		check(soloud.getActiveVoiceCount() == 300, "300 voices are active");
	}

	float play3dVolume(Soloud &aSoloud, Wav &aWav)
	{
		handle h = aSoloud.play3d(aWav, 1.0f, 0.0f, 0.0f);
		float volume = aSoloud.getOverallVolume(h);
		aSoloud.stop(h);
		return volume;
	}

	// #361: AudioSourceInstance3dData::init kept the previous voice's flags,
	// so a reused slot could compute play3d attenuation as listener-relative.
	void play3d_reused_slot_flags()
	{
		Wav world;
		check(loadWav(world, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		world.set3dAttenuation(AudioSource::INVERSE_DISTANCE, 1.0f);
		Wav relative;
		check(loadWav(relative, makeConstantWav(1, 4410, 0.25f)) == SO_NO_ERROR, "wav loads");
		relative.set3dAttenuation(AudioSource::INVERSE_DISTANCE, 1.0f);
		relative.set3dListenerRelative(true);

		Soloud fresh;
		check(initNull(fresh), "null driver init");
		fresh.set3dListenerPosition(100.0f, 0.0f, 0.0f);
		float expected = play3dVolume(fresh, world);

		Soloud reused;
		check(initNull(reused), "null driver init");
		reused.set3dListenerPosition(100.0f, 0.0f, 0.0f);
		handle r = reused.play3d(relative, 1.0f, 0.0f, 0.0f);
		reused.update3dAudio();
		reused.stop(r);
		float actual = play3dVolume(reused, world);
		std::printf("  fresh slot volume %g, reused slot volume %g\n", expected, actual);
		check(std::fabs(expected - actual) < 1e-6f, "reused slot gets the same 3D volume");
	}

	// An inaudible voice that ticks wrapped its loop without checking that
	// the wrap gave any audio (the mixing path stops when it gives none), so
	// a looping sound whose loop point is past its end, made inaudible by 3D
	// attenuation, spun forever in mix.
	void inaudible_tick_empty_loop_ends()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(1, 1000, 0.25f)) == SO_NO_ERROR, "wav loads");
		wav.setLooping(true);
		wav.setLoopPoint(1.0);
		wav.setInaudibleBehavior(true, false);
		wav.set3dAttenuation(AudioSource::INVERSE_DISTANCE, 1.0f);
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle h = soloud.play3d(wav, 0.0f, 0.0f, -5000.0f);
		render(soloud, 4096);
		check(soloud.isValidVoiceHandle(h), "voice keeps looping");
	}

	// An inaudible voice that ticks left the rest of a short block as it was,
	// holding the audio read two blocks before; made audible again during
	// that block, the voice played that audio after its end.
	void inaudible_tick_clears_short_block()
	{
		Wav wav;
		check(loadWav(wav, makeConstantWav(1, 1300, 0.5f)) == SO_NO_ERROR, "wav loads");
		wav.setInaudibleBehavior(true, false);
		Soloud soloud;
		check(soloud.init(0, Soloud::NULLDRIVER, kRate, 2048, 2) == SO_NO_ERROR, "null driver init");
		handle h = soloud.play3d(wav, 0.0f, 0.0f, -1.0f);
		std::vector<float> out(2 * 1000);
		soloud.mix(out.data(), 1000);
		// Inaudible while the last block (frames 1024 to 1299) is read.
		soloud.setVolume(h, 0.0f);
		soloud.update3dAudio();
		soloud.mix(out.data(), 100);
		soloud.setVolume(h, 1.0f);
		soloud.update3dAudio();
		soloud.mix(out.data(), 400);
		// The linear resampler plays frame 1299 one frame late, at 1300.
		check(out[2 * (1300 - 1100)] != 0.0f, "the last frame is heard");
		float after = 0.0f;
		for (size_t i = 2 * (1301 - 1100); i < 2 * 400; i++)
			after = std::fmax(after, std::fabs(out[i]));
		std::printf("  peak after the end %g\n", after);
		check(after == 0.0f, "nothing after the end of the sound");
	}

	// Noise divides its sum by the sum of the octave weights: weights summing
	// to 0 gave infinity, then NaN, which silenced the whole mix.
	void noise_zero_octave_sum_is_silent()
	{
		Noise noise;
		noise.setOctaveScale(1, -1, 0, 0, 0, 0, 0, 0, 0, 0);
		Soloud soloud;
		check(soloud.init(0, Soloud::NULLDRIVER, kRate, 2048, 2) == SO_NO_ERROR, "null driver init");
		soloud.play(noise);
		std::vector<float> out(2 * 1024);
		soloud.mix(out.data(), 1024);
		bool silent = true;
		for (size_t i = 0; i < out.size(); i++)
			silent = silent && out[i] == 0.0f;
		check(silent, "weights summing to 0 give silence");
	}

	// WavStream::load* replaced the stream's data without stopping its
	// voices, which went on reading the freed memory file. Wav's loaders
	// stop first.
	void wavstream_reload_stops_voices()
	{
		WavStream stream;
		check(loadStream(stream, makeConstantWav(1, 44100, 0.25f)) == SO_NO_ERROR, "stream loads");
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		handle first = soloud.play(stream);
		render(soloud, 1024);
		check(loadStream(stream, makeConstantWav(2, 44100, 0.5f)) == SO_NO_ERROR, "stream reloads");
		check(!soloud.isValidVoiceHandle(first), "reloading stops the stream's voices");
		handle second = soloud.play(stream);
		render(soloud, 1024);
		check(loadStream(stream, std::vector<unsigned char>(64, 0)) != SO_NO_ERROR, "garbage fails to load");
		check(!soloud.isValidVoiceHandle(second), "a failed reload stops them too");
		check(soloud.getActiveVoiceCount() == 0, "no voice reads the old data");
	}

	// Vic::setRegister and getRegister took any index: past the 4 registers
	// they wrote and read the noise table that follows them, and before
	// them the clocks.
	void vic_register_index_in_range()
	{
		Vic vic;
		unsigned char noise[4];
		float clocks[4];
		std::memcpy(noise, vic.m_noise, sizeof(noise));
		std::memcpy(clocks, vic.m_clocks, sizeof(clocks));
		for (int reg = Vic::MAX_REGS; reg < Vic::MAX_REGS + 4; reg++)
			vic.setRegister(reg, (unsigned char)(vic.m_noise[reg - Vic::MAX_REGS] ^ 0xff));
		for (int reg = -16; reg < 0; reg++)
			vic.setRegister(reg, 0x5a);
		check(std::memcmp(noise, vic.m_noise, sizeof(noise)) == 0, "the noise table is untouched");
		check(std::memcmp(clocks, vic.m_clocks, sizeof(clocks)) == 0, "the clocks are untouched");
		check(vic.getRegister(Vic::MAX_REGS) == 0 && vic.getRegister(-1) == 0, "out-of-range registers read 0");
		vic.setRegister(Vic::NOISE, 200);
		check(vic.getRegister(Vic::NOISE) == 200, "registers in range still work");
	}

	// An sfxr version 102 file: the version, a wave type, 22 floats with
	// the filter flag byte after the 14th, then 2 more floats.
	std::vector<unsigned char> sfxrFile(unsigned char aFilterByte)
	{
		std::vector<unsigned char> out;
		int head[2] = { 102, 1 };
		const unsigned char *bytes = (const unsigned char *)head;
		out.insert(out.end(), bytes, bytes + sizeof(head));
		for (int i = 0; i < 24; i++)
		{
			if (i == 14)
				out.push_back(aFilterByte);
			float value = 0.05f * (float)(i + 1);
			bytes = (const unsigned char *)&value;
			out.insert(out.end(), bytes, bytes + sizeof(value));
		}
		return out;
	}

	// Sfxr::loadParamsFile read field after field and succeeded whatever
	// the file held, so a short file loaded with some settings changed and
	// the rest kept.
	void sfxr_short_file_fails()
	{
		std::vector<unsigned char> bytes = sfxrFile(0);
		Sfxr sfxr;
		check(sfxr.loadParamsMem(bytes.data(), (unsigned int)bytes.size(), true, false) == SO_NO_ERROR, "a whole file loads");
		check(sfxr.mParams.wave_type == 1 && sfxr.mParams.p_arp_mod == 0.05f * 24, "every field is read");
		Sfxr fresh;
		bytes.resize(bytes.size() - 3);
		check(fresh.loadParamsMem(bytes.data(), (unsigned int)bytes.size(), true, false) == FILE_LOAD_FAILED, "a short file fails");
		check(fresh.mParams.wave_type == 0 && fresh.mParams.p_base_freq == 0.3f && fresh.mParams.sound_vol == 0.5f,
			"a short file changes nothing");
	}

	// Sfxr::loadParamsFile read the filter flag's byte straight into a bool,
	// so a byte other than 0 or 1 made a bool that is neither.
	void sfxr_filter_flag_is_a_bool()
	{
		std::vector<unsigned char> bytes = sfxrFile(2);
		Sfxr sfxr;
		check(sfxr.loadParamsMem(bytes.data(), (unsigned int)bytes.size(), true, false) == SO_NO_ERROR, "the file loads");
		unsigned char raw = 0xff;
		std::memcpy(&raw, &sfxr.mParams.filter_on, 1);
		check(raw == 1, "a nonzero flag byte reads as true");
	}

	// SfxrInstance::getAudio went on after the sound's end when called
	// again, as the generic seek does while discarding: its envelope then
	// read env_length[3], past the array.
	void sfxr_ended_sound_stays_silent()
	{
		Sfxr sfxr;
		sfxr.loadPreset(Sfxr::BLIP, 3);
		AudioSourceInstance *instance = sfxr.createInstance();
		instance->init(sfxr, 0);
		std::vector<float> buffer(512);
		unsigned int total = 0;
		for (int i = 0; i < 1000 && !instance->hasEnded(); i++)
			total += instance->getAudio(buffer.data(), 512, 512);
		check(instance->hasEnded() && total > 0, "the sound ends");
		check(instance->getAudio(buffer.data(), 512, 512) == 0, "an ended sound gives no more audio");
		delete instance;
	}

	// A Monotone cell: a note (1 to 126, 127 ends the note, 0 keeps the
	// last), an effect (0 to 7) and its 6 bits of data.
	unsigned short monotoneCell(unsigned int aNote, unsigned int aEffect = 0, unsigned int aData = 0)
	{
		return (unsigned short)((aNote << 9) | (aEffect << 6) | aData);
	}

	// A Monotone song titled "Test": aPatterns patterns of 64 rows of
	// aTracks cells, the first cells from aCells and the rest 0, played in
	// aOrder (the remaining order entries are 0xff).
	std::vector<unsigned char> monotoneSong(unsigned int aPatterns, unsigned int aTracks,
		const std::vector<unsigned char> &aOrder, const std::vector<unsigned short> &aCells)
	{
		static const char magic[] = "\bMONOTONE";
		std::vector<unsigned char> out(magic, magic + 9);
		unsigned char title[41] = { 4, 'T', 'e', 's', 't' };
		unsigned char comment[41] = { 0 };
		out.insert(out.end(), title, title + 41);
		out.insert(out.end(), comment, comment + 41);
		out.push_back(1);
		out.push_back((unsigned char)aPatterns);
		out.push_back((unsigned char)aTracks);
		out.push_back(2);
		for (unsigned int i = 0; i < 256; i++)
			out.push_back(i < aOrder.size() ? aOrder[i] : 0xff);
		for (unsigned int i = 0; i < aPatterns * 64 * aTracks; i++)
		{
			unsigned short cell = i < aCells.size() ? aCells[i] : 0;
			out.push_back((unsigned char)(cell & 0xff));
			out.push_back((unsigned char)(cell >> 8));
		}
		return out;
	}

	result loadMonotone(Monotone &aSong, const std::vector<unsigned char> &aBytes)
	{
		return aSong.loadMem(aBytes.data(), (unsigned int)aBytes.size(), true, false);
	}

	// aFrames of a new voice of aSong read straight from its instance, in
	// 512-frame calls, at aRate.
	std::vector<float> monotoneAudio(Monotone &aSong, unsigned int aFrames, float aRate = 44100.0f)
	{
		AudioSourceInstance *instance = aSong.createInstance();
		instance->init(aSong, 0);
		instance->mSamplerate = aRate;
		std::vector<float> out(aFrames, 0.0f);
		for (unsigned int f = 0; f < aFrames; f += 512)
			instance->getAudio(&out[f], aFrames - f < 512 ? aFrames - f : 512, 512);
		delete instance;
		return out;
	}

	// A Monotone built over bytes of 0xab, so members its constructor does
	// not set hold garbage that is the same on every run.
	Monotone *newMonotoneOverGarbage()
	{
		void *memory = std::malloc(sizeof(Monotone));
		std::memset(memory, 0xab, sizeof(Monotone));
		return new (memory) Monotone();
	}

	void deleteMonotoneOverGarbage(Monotone *aSong)
	{
		aSong->~Monotone();
		std::free(aSong);
	}

	// Monotone's constructor left the song's track count and order
	// uninitialised, and a failed load kept the track count it had read
	// (or the previous song's) without any pattern data: playing either
	// read through a null pointer.
	void monotone_empty_song_is_silent()
	{
		Monotone *song = newMonotoneOverGarbage();
		std::vector<float> out = monotoneAudio(*song, 4096);
		check(maxAbsChannel(out, 0, 1) == 0, "a song never loaded is silent");
		std::vector<unsigned char> bytes = monotoneSong(1, 1, { 0 }, { monotoneCell(40) });
		check(loadMonotone(*song, bytes) == SO_NO_ERROR, "a song loads");
		out = monotoneAudio(*song, 4096);
		check(maxAbsChannel(out, 0, 1) > 0.1f, "the song plays");
		bytes[9 + 41 + 41] = 2; // version
		check(loadMonotone(*song, bytes) == FILE_LOAD_FAILED, "version 2 fails");
		check(song->mSong.mTotalTracks == 0 && song->mSong.mPatternData == NULL, "a failed load leaves no song");
		out = monotoneAudio(*song, 4096);
		check(maxAbsChannel(out, 0, 1) == 0, "a failed load is silent");
		deleteMonotoneOverGarbage(song);
	}

	// Monotone::loadFile ignored how many bytes each read gave, so a short
	// file loaded with the rest of its bytes taken from earlier reads (or
	// uninitialised ones).
	void monotone_short_file_fails()
	{
		std::vector<unsigned char> bytes = monotoneSong(2, 3, { 0, 1 }, { monotoneCell(40), monotoneCell(50) });
		Monotone song;
		check(loadMonotone(song, bytes) == SO_NO_ERROR, "a whole song loads");
		const unsigned int cuts[] = { 1, 8, 9, 49, 50, 91, 95, 350, 351, 352, (unsigned int)bytes.size() - 1 };
		for (unsigned int cut : cuts)
		{
			std::vector<unsigned char> part(bytes.begin(), bytes.begin() + cut);
			check(loadMonotone(song, part) == FILE_LOAD_FAILED, "a short song fails");
			check(song.mSong.mTotalTracks == 0, "a short song leaves no song");
		}
	}

	// Monotone::loadFile ended a Pascal title or comment at the length in
	// its first byte, up to 255, in a 200-byte buffer holding 41 bytes from
	// the file: a length over 40 read stale bytes, and over 198 wrote past
	// the buffer.
	void monotone_long_title_is_cut()
	{
		std::vector<unsigned char> bytes = monotoneSong(1, 1, { 0 }, {});
		bytes[9] = 255;
		std::memset(&bytes[10], 'T', 40);
		bytes[50] = 41;
		std::memset(&bytes[51], 'C', 40);
		Monotone song;
		check(loadMonotone(song, bytes) == SO_NO_ERROR, "the song loads");
		check(std::strlen(song.mSong.mTitle) == 40 && song.mSong.mTitle[39] == 'T', "the title is its 40 bytes");
		check(std::strlen(song.mSong.mComment) == 40 && song.mSong.mComment[39] == 'C', "the comment is its 40 bytes");
	}

	// Voices keep 12 channels and 12 hardware channels, but a song could
	// have up to 255 tracks and setParams took any number of hardware
	// channels: both overran the arrays.
	void monotone_at_most_12_channels()
	{
		Monotone song;
		check(loadMonotone(song, monotoneSong(1, 12, { 0 }, {})) == SO_NO_ERROR, "12 tracks load");
		check(loadMonotone(song, monotoneSong(1, 13, { 0 }, {})) == FILE_LOAD_FAILED, "13 tracks fail");
		check(song.setParams(12) == SO_NO_ERROR, "12 hardware channels are allowed");
		check(song.setParams(13) == INVALID_PARAMETER, "13 hardware channels are refused");
		check(song.mHardwareChannels == 12, "a refused count changes nothing");
	}

	// MonotoneInstance::getAudio indexed mNotesHz (800 entries) with notes
	// up to 126 times 8, read the order past its 256 entries when the song
	// had no end marker, and read patterns the song does not have.
	void monotone_indices_in_range()
	{
		Monotone high;
		check(loadMonotone(high, monotoneSong(1, 1, { 0 }, { monotoneCell(120) })) == SO_NO_ERROR, "note 120 loads");
		Monotone top;
		check(loadMonotone(top, monotoneSong(1, 1, { 0 }, { monotoneCell(100) })) == SO_NO_ERROR, "note 100 loads");
		std::vector<float> a = monotoneAudio(high, 8192);
		std::vector<float> b = monotoneAudio(top, 8192);
		check(a == b, "notes past the table play its last entry");

		// Every order entry is pattern 0; at 60 Hz a tick is one sample.
		std::vector<unsigned char> order(256, 0);
		Monotone endless;
		check(loadMonotone(endless, monotoneSong(1, 1, order, { monotoneCell(40) })) == SO_NO_ERROR, "the song loads");
		MonotoneInstance *instance = (MonotoneInstance *)endless.createInstance();
		instance->init(endless, 0);
		instance->mSamplerate = 60.0f;
		std::vector<float> buffer(512);
		bool inRange = true;
		for (int i = 0; i < 300 && inRange; i++)
		{
			instance->getAudio(buffer.data(), 512, 512);
			inRange = instance->mOrder >= 0 && instance->mOrder < 256;
		}
		check(inRange, "the order position stays in the order");
		delete instance;

		Monotone missing;
		check(loadMonotone(missing, monotoneSong(1, 1, { 3 }, { monotoneCell(40) })) == SO_NO_ERROR, "the song loads");
		Monotone empty;
		check(loadMonotone(empty, monotoneSong(1, 1, { 0 }, {})) == SO_NO_ERROR, "the song loads");
		check(monotoneAudio(missing, 8192) == monotoneAudio(empty, 8192), "a missing pattern plays as empty rows");
	}

	// Monotone's constructor filled mNotesHz down to entry 2 only; vibrato
	// on note 1 reads entry 1.
	void monotone_note_table_is_filled()
	{
		Monotone *song = newMonotoneOverGarbage();
		const float interval = 1.00724641222f;
		float hz = 27.5f;
		for (int i = 0; i < 7; i++)
			hz = hz / interval;
		check(song->mNotesHz[1] == (int)std::floor(hz + 0.5f), "entry 1 continues the table");
		check(song->mNotesHz[1] <= song->mNotesHz[2], "the table rises");
		deleteMonotoneOverGarbage(song);
	}

	// MonotoneInstance::getAudio took floor(rate / 60) samples per tick, so
	// below 60 Hz (a slow play speed) it divided by zero.
	void monotone_slow_rate_does_not_divide_by_zero()
	{
		Monotone song;
		check(loadMonotone(song, monotoneSong(1, 1, { 0 }, { monotoneCell(40) })) == SO_NO_ERROR, "the song loads");
		MonotoneInstance *instance = (MonotoneInstance *)song.createInstance();
		instance->init(song, 0);
		instance->mSamplerate = 30.0f;
		std::vector<float> buffer(512);
		check(instance->getAudio(buffer.data(), 300, 512) == 300, "the voice plays");
		// 4 ticks a row: 75 rows, the 11th of the second pattern.
		check(instance->mRow == 11, "rows advance a tick a sample");
		delete instance;
	}

	// MonotoneInstance's constructor left each channel's frequencies
	// uninitialised, and a portamento to a note on a channel's first note
	// slides from the frequency it had.
	void monotone_first_slide_starts_at_zero()
	{
		Monotone song;
		check(loadMonotone(song, monotoneSong(1, 1, { 0 }, { monotoneCell(40, 3, 5) })) == SO_NO_ERROR, "the song loads");
		void *memory = std::malloc(sizeof(MonotoneInstance));
		std::memset(memory, 0xab, sizeof(MonotoneInstance));
		MonotoneInstance *instance = new (memory) MonotoneInstance(&song);
		instance->init(song, 0);
		instance->mSamplerate = 60.0f;
		std::vector<float> buffer(4);
		// The first row is read on the 4th tick, a sample each at 60 Hz.
		instance->getAudio(buffer.data(), 4, 4);
		check(instance->mChannel[0].mFreq[0] == 0 && instance->mChannel[0].mPortamento == 5,
			"the slide starts from 0 Hz");
		instance->~MonotoneInstance();
		std::free(memory);
	}

	// A register write and a wait of a chip dump.
	unsigned short dumpWrite(unsigned int aReg, unsigned int aValue)
	{
		return (unsigned short)(aReg << 8 | aValue);
	}

	unsigned short dumpWait(unsigned int aClocks)
	{
		return (unsigned short)(0x8000 | aClocks);
	}

	// A CHIPTUNE dump's 28-byte header: data at aDataOffset, aLength bytes
	// of ops in 1024-byte chunks, looping to aLoopPos.
	std::vector<unsigned char> chipHeader(unsigned char aChip, unsigned char aFlags, unsigned int aLength,
		unsigned int aLoopPos = 0, unsigned int aDataOffset = 28)
	{
		std::vector<unsigned char> out;
		const char magic[] = "CHIPTUNE";
		out.insert(out.end(), magic, magic + 8);
		unsigned int kchunks = (aLength + 1023) / 1024;
		unsigned int lastchunk = aLength - (kchunks ? kchunks - 1 : 0) * 1024;
		const unsigned short words[] = { (unsigned short)aDataOffset, (unsigned short)(aChip | aFlags << 8),
			(unsigned short)kchunks, (unsigned short)lastchunk, (unsigned short)(aLoopPos / 1024),
			(unsigned short)(aLoopPos % 1024) };
		for (unsigned short word : words)
		{
			out.push_back((unsigned char)(word & 0xff));
			out.push_back((unsigned char)(word >> 8));
		}
		const unsigned int speeds[] = { 50, 985248 };
		for (unsigned int speed : speeds)
			for (int i = 0; i < 4; i++)
				out.push_back((unsigned char)(speed >> (8 * i)));
		return out;
	}

	// An uncompressed dump (flags bit 0) of aOps.
	std::vector<unsigned char> chipDump(unsigned char aChip, unsigned char aFlags, const std::vector<unsigned short> &aOps,
		unsigned int aLoopPos = 0)
	{
		std::vector<unsigned char> out = chipHeader(aChip, aFlags | 1, (unsigned int)aOps.size() * 2, aLoopPos);
		for (unsigned short op : aOps)
		{
			out.push_back((unsigned char)(op & 0xff));
			out.push_back((unsigned char)(op >> 8));
		}
		return out;
	}

	// A SID tune: a triangle with an envelope, then a pulse.
	std::vector<unsigned short> sidTune()
	{
		return { dumpWrite(24, 0x0f), dumpWrite(5, 0x22), dumpWrite(6, 0xa8), dumpWrite(0, 0x00), dumpWrite(1, 0x1c),
			dumpWrite(4, 0x11), dumpWait(2000), dumpWrite(4, 0x10), dumpWait(2000), dumpWrite(1, 0x25),
			dumpWrite(4, 0x41), dumpWrite(2, 0x00), dumpWrite(3, 0x08), dumpWait(3000) };
	}

	result loadTedSid(TedSid &aTune, const std::vector<unsigned char> &aBytes)
	{
		return aTune.loadMem(aBytes.data(), (unsigned int)aBytes.size(), true, false);
	}

	// aFrames of a new voice of aSource, read straight from its instance at
	// its own rate.
	std::vector<float> sourceAudio(AudioSource &aSource, unsigned int aFrames)
	{
		AudioSourceInstance *instance = aSource.createInstance();
		instance->init(aSource, 0);
		std::vector<float> out(aFrames, 0.0f);
		for (unsigned int f = 0; f < aFrames; f += 512)
			instance->getAudio(&out[f], aFrames - f < 512 ? aFrames - f : 512, 512);
		delete instance;
		return out;
	}

	// TedSid::loadFile picked the SID model from the source's own flags
	// (mFlags, whose bits 4 and 5 are 3D settings) instead of the dump's.
	void tedsid_model_from_dump()
	{
		TedSid r6581, r8580, relative;
		check(loadTedSid(r6581, chipDump(0, 0, sidTune())) == SO_NO_ERROR, "a 6581 dump loads");
		check(loadTedSid(r8580, chipDump(0, 16, sidTune())) == SO_NO_ERROR, "an 8580 dump loads");
		relative.set3dListenerRelative(true);
		check(loadTedSid(relative, chipDump(0, 0, sidTune())) == SO_NO_ERROR, "the dump loads");
		std::vector<float> a = sourceAudio(r6581, 20000);
		check(a != sourceAudio(r8580, 20000), "the dump's flags choose the model");
		check(a == sourceAudio(relative, 20000), "the source's 3D flags do not");
	}

	// TedSid::loadFile freed the ops its voices were reading.
	void tedsid_reload_stops_voices()
	{
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		TedSid tune;
		check(loadTedSid(tune, chipDump(0, 0, sidTune())) == SO_NO_ERROR, "the dump loads");
		handle h = soloud.play(tune);
		render(soloud, 1024);
		check(loadTedSid(tune, chipDump(0, 16, sidTune())) == SO_NO_ERROR, "the dump reloads");
		check(!soloud.isValidVoiceHandle(h), "reloading stops the voices");
	}

	// TedSid::loadFile took the header's sizes as they were: no chunks
	// asked new[] for a negative size, a loop position at or past the end
	// read past the ops, a data offset past the file's end read a negative
	// length, a short file kept uninitialised ops, and the two halves of
	// the loop position were read in either order.
	void tedsid_header_is_checked()
	{
		TedSid tune;
		std::vector<unsigned short> ops = sidTune();
		std::vector<unsigned char> bytes = chipDump(0, 0, ops);
		bytes[12] = bytes[13] = 0; // no chunks
		check(loadTedSid(tune, bytes) == FILE_LOAD_FAILED, "a dump of no chunks fails");
		check(loadTedSid(tune, chipDump(0, 0, ops, (unsigned int)ops.size() * 2)) == FILE_LOAD_FAILED,
			"a loop at the end fails");
		bytes = chipDump(0, 0, ops);
		bytes[8] = 0xff; // data offset 255
		check(loadTedSid(tune, bytes) == FILE_LOAD_FAILED, "data past the end fails");
		bytes = chipDump(0, 0, ops);
		bytes.resize(bytes.size() - 2);
		check(loadTedSid(tune, bytes) == FILE_LOAD_FAILED, "a short dump fails");
		check(loadTedSid(tune, chipDump(0, 0, ops, 1026)) == FILE_LOAD_FAILED, "a loop past the end fails");
		check(tune.mOps == NULL && tune.mLength == 0, "a failed load leaves no ops");
		// 1030 bytes of ops: the loop's chunk and offset are read in order.
		std::vector<unsigned short> long_ops(515, dumpWait(10));
		check(loadTedSid(tune, chipDump(0, 0, long_ops, 1026)) == SO_NO_ERROR, "a loop in the second chunk loads");
		check(tune.mLooppos == 1026, "the loop position is the chunk then the offset");
	}

	// TedSidInstance::tick reads ops until a wait, so a dump whose loop has
	// no wait (or only waits of 0) spun forever on the audio thread.
	void tedsid_loop_without_wait_fails()
	{
		TedSid tune;
		check(loadTedSid(tune, chipDump(0, 0, { dumpWrite(24, 15), dumpWrite(4, 0x11), dumpWrite(0, 3) })) == FILE_LOAD_FAILED,
			"a dump without waits fails");
		check(loadTedSid(tune, chipDump(0, 0, { dumpWait(100), dumpWrite(4, 0x11), dumpWait(0) }, 2)) == FILE_LOAD_FAILED,
			"a loop with only a wait of 0 fails");
		check(loadTedSid(tune, chipDump(0, 0, { dumpWrite(4, 0x11), dumpWait(1), dumpWrite(0, 3) }, 4)) == FILE_LOAD_FAILED,
			"a loop after the last wait fails");
		check(loadTedSid(tune, chipDump(0, 0, { dumpWrite(4, 0x11), dumpWait(1), dumpWrite(0, 3) }, 2)) == SO_NO_ERROR,
			"a loop through a wait loads");
	}

	// zx7_decompress read and wrote without bounds: a dump's compressed
	// chunks wrote past its ops and read past its data.
	void tedsid_zx7_stays_in_bounds()
	{
		// sidTune()'s 28 bytes, compressed.
		const unsigned char packed[] = { 0x0f, 0x00, 0x18, 0x22, 0x05, 0xa8, 0x06, 0x00, 0x00, 0x1c, 0x02, 0x01, 0x11,
			0x04, 0xd0, 0x87, 0x10, 0x80, 0x03, 0x25, 0x01, 0x41, 0x04, 0x00, 0x02, 0x08, 0x08, 0x03, 0xb8, 0x8b, 0x00, 0x04 };
		std::vector<unsigned char> bytes = chipHeader(0, 0, 28);
		bytes.insert(bytes.end(), packed, packed + sizeof(packed));
		TedSid packedTune, plainTune;
		check(loadTedSid(packedTune, bytes) == SO_NO_ERROR, "a compressed dump loads");
		check(loadTedSid(plainTune, chipDump(0, 0, sidTune())) == SO_NO_ERROR, "the dump loads");
		check(sourceAudio(packedTune, 8000) == sourceAudio(plainTune, 8000), "compressed and plain play the same");

		// 3001 zero bytes, compressed, for 28 bytes of ops.
		const unsigned char zeros[] = { 0x00, 0x80, 0x0b, 0xb7, 0x00, 0x80, 0x00, 0x40 };
		bytes = chipHeader(0, 0, 28);
		bytes.insert(bytes.end(), zeros, zeros + sizeof(zeros));
		bytes.resize(bytes.size() + 6, 0);
		check(loadTedSid(packedTune, bytes) == FILE_LOAD_FAILED, "a chunk longer than the ops fails");
		bytes = chipHeader(0, 0, 28);
		bytes.insert(bytes.end(), packed, packed + 20);
		check(loadTedSid(packedTune, bytes) == FILE_LOAD_FAILED, "a cut chunk fails");
	}

	// SIDsound::reset set each voice's envelope rate, an index into
	// RateCountPeriod, to RateCountPeriod[0] (0x7f00) itself: until a gate
	// set it, every envelope step read 0x7f00 entries past the table.
	void sid_envelope_rate_starts_in_range()
	{
		SIDsound sid(SID8580, 0);
		bool inRange = true;
		for (int i = 0; i < 3; i++)
			inRange = inRange && sid.voice[i].envCounterCompare < 16;
		check(inRange, "envelope rates start as indices");
		sid.setSampleRate(TED_SOUND_CLOCK);
		sid.setFrequency(1);
		short buffer[256];
		sid.calcSamples(buffer, 256);
		check(sid.read(0x1c) == 0, "a voice never gated stays silent");
	}

	// SIDsound's attack counted its envelope level past 0xff when a gate
	// came back at full level: the level went to 0x1ff, decay then read
	// envGenDRdivisors[0x1ff], past the table, and fell through the
	// sustain level. The level is now 8 bits, wrapping to 0 as in reSID.
	void sid_attack_from_full_level_wraps()
	{
		SIDsound sid(SID6581, 0);
		sid.setSampleRate(TED_SOUND_CLOCK);
		sid.setFrequency(1);
		short buffer[256];
		sid.write(24, 0x0f);
		sid.write(14, 0x00);
		sid.write(15, 0x10);
		sid.write(19, 0x00); // fastest attack and decay
		sid.write(20, 0xf0); // sustain at 0xff, fastest release
		sid.write(18, 0x11);
		for (int i = 0; i < 20; i++)
			sid.calcSamples(buffer, 256);
		check(sid.read(0x1c) == 0xff, "the envelope sustains at 0xff");
		sid.write(18, 0x10);
		sid.write(18, 0x11);
		bool eightBits = true;
		for (int i = 0; i < 40; i++)
		{
			sid.calcSamples(buffer, 256);
			eightBits = eightBits && sid.voice[2].envCurrLevel <= 0xff;
		}
		check(eightBits, "the level stays below 0x100");
		check(sid.read(0x1c) == 0xff, "a new attack from 0xff comes back to sustain at 0xff");
	}

	result loadAy(Ay &aAy, const std::vector<unsigned char> &aBytes)
	{
		return aAy.loadMem(aBytes.data(), (unsigned int)aBytes.size(), true, false);
	}

	// An uncompressed AY dump of aOps (chip type 1 unless given) at the given
	// CPU (tick) and chip speeds.
	std::vector<unsigned char> ayDump(const std::vector<unsigned short> &aOps, unsigned char aFlags = 0,
		unsigned int aCpuSpeed = 50, unsigned int aChipSpeed = 1774400, unsigned char aChip = 1)
	{
		std::vector<unsigned char> out = chipDump(aChip, aFlags, aOps);
		const unsigned int speeds[] = { aCpuSpeed, aChipSpeed };
		for (int s = 0; s < 2; s++)
			for (int i = 0; i < 4; i++)
				out[20 + 4 * s + i] = (unsigned char)(speeds[s] >> (8 * i));
		return out;
	}

	// An AY tune: channel A's tone and noise, then an envelope on channel C,
	// 9 ticks long.
	std::vector<unsigned short> ayTune()
	{
		return { dumpWrite(7, 0x36), dumpWrite(0, 0x80), dumpWrite(1, 0x01), dumpWrite(8, 0x0f), dumpWrite(6, 0x0c),
			dumpWait(3), dumpWrite(4, 0x40), dumpWrite(5, 0x02), dumpWrite(10, 0x10), dumpWrite(11, 0x00),
			dumpWrite(12, 0x04), dumpWrite(13, 0x0e), dumpWait(4), dumpWrite(8, 0x00), dumpWait(2) };
	}

	// aFrames (a multiple of 512) of a new voice of aAy, each 512-frame read
	// with its left then its right channel.
	std::vector<float> ayAudio(Ay &aAy, unsigned int aFrames)
	{
		AudioSourceInstance *instance = aAy.createInstance();
		instance->init(aAy, 0);
		std::vector<float> out, block(1024);
		for (unsigned int f = 0; f < aFrames; f += 512)
		{
			std::fill(block.begin(), block.end(), 0.0f);
			instance->getAudio(block.data(), 512, 512);
			out.insert(out.end(), block.begin(), block.end());
		}
		delete instance;
		return out;
	}

	// Frames a voice of aAy plays, read 512 at a time, until it ends.
	unsigned int ayFramesPlayed(Ay &aAy)
	{
		AudioSourceInstance *instance = aAy.createInstance();
		instance->init(aAy, 0);
		std::vector<float> block(1024);
		unsigned int frames = 0;
		for (int i = 0; i < 1000 && !instance->hasEnded(); i++)
			frames += instance->getAudio(block.data(), 512, 512);
		delete instance;
		return frames;
	}

	// AyInstance::getAudio wrote the right channel aSamplesToRead floats
	// after the left instead of aBufferSize: a short read put it inside the
	// left channel's buffer.
	void ay_right_channel_at_buffer_stride()
	{
		Ay ay;
		check(loadAy(ay, ayDump(ayTune())) == SO_NO_ERROR, "the dump loads");
		AudioSourceInstance *full = ay.createInstance();
		full->init(ay, 0);
		AudioSourceInstance *part = ay.createInstance();
		part->init(ay, 0);
		std::vector<float> a(1024, -1.0f), b(1024, -1.0f);
		full->getAudio(a.data(), 512, 512);
		part->getAudio(b.data(), 100, 512);
		bool same = true;
		for (int i = 0; i < 100; i++)
			same = same && b[i] == a[i] && b[512 + i] == a[512 + i];
		check(same, "a short read puts both channels where a full read does");
		check(b[100] == -1.0f, "a short read leaves the rest of the left channel");
		delete full;
		delete part;
	}

	// Ay::loadFile replaced the ops its voices were reading without stopping
	// them, and leaked the old ones.
	void ay_reload_stops_voices()
	{
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		Ay ay;
		check(loadAy(ay, ayDump(ayTune())) == SO_NO_ERROR, "the dump loads");
		handle h = soloud.play(ay);
		render(soloud, 1024);
		check(soloud.isValidVoiceHandle(h), "the voice plays");
		check(loadAy(ay, ayDump(ayTune(), 64)) == SO_NO_ERROR, "the dump reloads");
		check(!soloud.isValidVoiceHandle(h), "reloading stops the voices");
	}

	// Ay::loadFile took the header's sizes as they were (as TedSid did): a
	// data offset past the file's end read a negative length, a short file
	// kept uninitialised ops, the two halves of the loop position were read
	// in either order, and no chunks asked new[] for a negative size.
	void ay_header_is_checked()
	{
		Ay ay;
		std::vector<unsigned short> ops = ayTune();
		std::vector<unsigned char> bytes = ayDump(ops);
		bytes[8] = 0xff; // data offset 255
		check(loadAy(ay, bytes) == FILE_LOAD_FAILED, "data past the end fails");
		bytes = ayDump(ops);
		bytes.resize(bytes.size() - 2);
		check(loadAy(ay, bytes) == FILE_LOAD_FAILED, "a short dump fails");
		check(ay.mOps == NULL && ay.mLength == 0, "a failed load leaves no ops");
		// Ay does not loop to the loop position, so one at the end is kept.
		bytes = ayDump(ops);
		bytes[18] = (unsigned char)(ops.size() * 2);
		check(loadAy(ay, bytes) == SO_NO_ERROR, "a loop position at the end loads");
		std::vector<unsigned short> long_ops(515, dumpWait(1));
		bytes = ayDump(long_ops);
		bytes[16] = 1; // the loop at chunk 1, offset 2
		bytes[18] = 2;
		check(loadAy(ay, bytes) == SO_NO_ERROR, "a dump of two chunks loads");
		check(ay.mLooppos == 1026, "the loop position is the chunk then the offset");
		bytes = ayDump(ops);
		bytes[12] = bytes[13] = 0; // no chunks
		check(loadAy(ay, bytes) == FILE_LOAD_FAILED, "a dump of no chunks fails");
	}

	// Ay::loadFile decompressed zx7 chunks without bounds, as TedSid did.
	void ay_zx7_stays_in_bounds()
	{
		// ayTune()'s 30 bytes, compressed.
		const unsigned char packed[] = { 0x36, 0x00, 0x07, 0x80, 0x00, 0x01, 0x01, 0x0f, 0x08, 0x0c, 0x00, 0x06, 0x03,
			0x80, 0x40, 0x04, 0x02, 0x05, 0x10, 0x00, 0x0a, 0x00, 0x0b, 0x04, 0x0c, 0x0e, 0x0d, 0x04, 0xc4, 0x16, 0x08,
			0x02, 0x80, 0x00, 0x02 };
		std::vector<unsigned char> plain = ayDump(ayTune());
		std::vector<unsigned char> bytes(plain.begin(), plain.begin() + 28);
		bytes[11] = 0; // compressed
		bytes.insert(bytes.end(), packed, packed + sizeof(packed));
		Ay packedTune, plainTune;
		check(loadAy(packedTune, bytes) == SO_NO_ERROR, "a compressed dump loads");
		check(loadAy(plainTune, plain) == SO_NO_ERROR, "the dump loads");
		check(ayAudio(packedTune, 4096) == ayAudio(plainTune, 4096), "compressed and plain play the same");

		// 3001 zero bytes, compressed, for 30 bytes of ops.
		const unsigned char zeros[] = { 0x00, 0x80, 0x0b, 0xb7, 0x00, 0x80, 0x00, 0x40 };
		std::vector<unsigned char> over(bytes.begin(), bytes.begin() + 28);
		over.insert(over.end(), zeros, zeros + sizeof(zeros));
		over.resize(over.size() + 6, 0);
		check(loadAy(packedTune, over) == FILE_LOAD_FAILED, "a chunk longer than the ops fails");
		std::vector<unsigned char> cut(bytes.begin(), bytes.begin() + 28 + 20);
		check(loadAy(packedTune, cut) == FILE_LOAD_FAILED, "a cut chunk fails");
	}

	// ChipPlayer divides by the CPU (tick) speed and the chip's speed / 8,
	// and renders frames of (CPU speed * 8172 / 44100) / 2 ticks: a speed of
	// 0 divided by zero, below 11 Hz a frame of 0 ticks never advanced,
	// spinning on the audio thread, and a chip ticking slower than the
	// samples (below 8 * 44100 Hz) rendered a tick's samples at once, which
	// could overrun the buffer.
	void ay_speeds_are_checked()
	{
		Ay ay;
		check(loadAy(ay, ayDump(ayTune(), 0, 0)) == FILE_LOAD_FAILED, "a CPU speed of 0 fails");
		check(loadAy(ay, ayDump(ayTune(), 0, 10)) == FILE_LOAD_FAILED, "a CPU speed of 10 fails");
		check(loadAy(ay, ayDump(ayTune(), 0, 50, 7)) == FILE_LOAD_FAILED, "a chip speed of 7 fails");
		check(loadAy(ay, ayDump(ayTune(), 0, 50, 8 * 44100 - 1)) == FILE_LOAD_FAILED, "a chip slower than the samples fails");
		check(loadAy(ay, ayDump(ayTune(), 0, 11, 8 * 44100)) == SO_NO_ERROR, "the slowest speeds load");
		check(ay.mCpuspeed == 11 && ay.mChipspeed == 8 * 44100, "the speeds are kept");
		check(ayFramesPlayed(ay) > 0, "the slowest speeds play");
	}

	// SNDCHIP's constructor left its tone, noise and envelope counters, the
	// noise output, the chip type and the port registers uninitialised: each
	// voice's first period came from whatever memory the chip was built in.
	void ay_chip_starts_at_zero()
	{
		SNDBUFFER buffer(8192);
		void *memory = std::malloc(sizeof(SNDCHIP));
		std::memset(memory, 0xa5, sizeof(SNDCHIP));
		SNDCHIP *chip = new (memory) SNDCHIP(buffer);
		check(chip->ta == 0 && chip->tb == 0 && chip->tc == 0 && chip->tn == 0 && chip->te == 0, "counters start at 0");
		check(chip->bitN == 0, "the noise output starts at 0");
		check(chip->reg[14] == 0 && chip->reg[15] == 0, "the port registers start at 0");
		chip->~SNDCHIP();
		std::free(memory);
	}

	// ChipPlayer::play stopped at the end of the ops before playing the op it
	// had just read, so a dump's last register write was dropped.
	void ay_last_write_is_played()
	{
		Ay ay;
		check(loadAy(ay, ayDump({ dumpWrite(8, 0x0f), dumpWait(2), dumpWrite(8, 0x03) })) == SO_NO_ERROR, "the dump loads");
		AudioSourceInstance *instance = ay.createInstance();
		instance->init(ay, 0);
		std::vector<float> block(1024);
		for (int i = 0; i < 8 && !instance->hasEnded(); i++)
			instance->getAudio(block.data(), 512, 512);
		check(instance->hasEnded(), "the voice ends");
		check(instance->getInfo(8) == 3.0f, "the last write is played");
		delete instance;
	}

	// ChipPlayer::play renders frames into an 8192-sample buffer, but played
	// a wait whole within a frame: a wait of more than about 8000 samples
	// overran the buffer onto its own samples (and rendered all of it in one
	// read on the audio thread).
	void ay_long_wait_is_spread()
	{
		Ay ay;
		// A steady tone for one wait of 60 ticks at 50 Hz: 52920 frames.
		check(loadAy(ay, ayDump({ dumpWrite(7, 0x3e), dumpWrite(0, 0x40), dumpWrite(8, 0x0f), dumpWait(60), dumpWrite(8, 0) })) ==
			SO_NO_ERROR, "the dump loads");
		unsigned int frames = ayFramesPlayed(ay);
		check(frames >= 52900 && frames <= 53000, "the whole wait plays");
		std::vector<float> audio = ayAudio(ay, 51200);
		float low = 1e9f, high = -1e9f;
		for (int i = 0; i < 512; i++)
		{
			float sample = audio[99 * 1024 + i];
			low = sample < low ? sample : low;
			high = sample > high ? sample : high;
		}
		check(high - low > 0.1f, "the tone plays through the wait");
	}

	// AyInstance::hasEnded turned true once the ops were read, while up to a
	// frame of samples was still in the buffer: every dump's end was cut.
	void ay_tail_is_played()
	{
		Ay ay;
		// 6 ticks at 50 Hz: 5292 frames.
		check(loadAy(ay, ayDump({ dumpWrite(7, 0x3e), dumpWrite(0, 0x40), dumpWrite(8, 0x0f), dumpWait(3), dumpWrite(0, 0x60),
			dumpWait(3) })) == SO_NO_ERROR, "the dump loads");
		unsigned int frames = ayFramesPlayed(ay);
		check(frames >= 5290 && frames <= 5300, "the whole dump plays");
	}

	// AyInstance::rewind only went back to the first op: the chips kept their
	// registers, counters and envelopes, and the buffer its samples, so a
	// looped or rewound voice did not play the dump from its start.
	void ay_rewind_restarts()
	{
		Ay ay;
		check(loadAy(ay, ayDump(ayTune())) == SO_NO_ERROR, "the dump loads");
		AudioSourceInstance *instance = ay.createInstance();
		instance->init(ay, 0);
		std::vector<float> block(1024), again;
		for (int i = 0; i < 3; i++)
			instance->getAudio(block.data(), 512, 512);
		check(instance->rewind() == SO_NO_ERROR, "the voice rewinds");
		for (int i = 0; i < 4; i++)
		{
			std::fill(block.begin(), block.end(), 0.0f);
			instance->getAudio(block.data(), 512, 512);
			again.insert(again.end(), block.begin(), block.end());
		}
		delete instance;
		check(again == ayAudio(ay, 2048), "a rewound voice plays as a new one");
	}

	// Planar raw samples, every channel set to aValue.
	result loadConstantRaw(Wav &aWav, unsigned int aChannels, unsigned int aFrames, float aValue)
	{
		std::vector<float> data(aChannels * aFrames, aValue);
		return aWav.loadRawWave(data.data(), (unsigned int)data.size(), (float)kRate, aChannels, true, false);
	}

	// Soloud::seek on a bus voice discards audio by mixing the bus's voices
	// in passes of the engine's scratch size divided by the bus's channels,
	// with that many frames as the channel stride. Strides that are not a
	// multiple of 16 hit panAndExpand's alignment assert (a breakpoint in
	// MSVC builds) or fault in its SSE loads, and a voice with more channels
	// than the bus overran the bus's one-block scratch.
	void bus_seek_mixes_in_blocks()
	{
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		Wav stereo;
		check(loadConstantRaw(stereo, 2, 30000, 0.25f) == SO_NO_ERROR, "raw wave loads");
		Bus mono;
		check(mono.setChannels(1) == SO_NO_ERROR, "mono bus");
		Bus wide;
		handle monoBus = soloud.play(mono);
		handle wideBus = soloud.play(wide);
		check(mono.play(stereo, 1) != 0 && wide.play(stereo, 1) != 0, "voices play through the buses");
		// A bus only mixes voices already in the active set.
		std::vector<float> out(2 * 4096);
		soloud.mix(out.data(), 512);
		// 21538 and 8308 frames: passes of 4096 and 2048 frames, then 1058
		// and 116.
		check(soloud.seek(monoBus, 0.5) == SO_NO_ERROR, "mono bus seeks");
		check(soloud.seek(wideBus, 0.2) == SO_NO_ERROR, "stereo bus seeks");
		soloud.mix(out.data(), 4096);
		check(allFinite(out), "output stays finite");
		soloud.mix(out.data(), 4096);
		soloud.mix(out.data(), 4096);
		soloud.mix(out.data(), 4096);
		// 30000 frames: the mono bus's voice is past its end after 22050 +
		// 16384 frames, the stereo bus's is not.
		check(mono.getActiveVoiceCount() == 0, "the seek advanced the mono bus's voice");
		check(wide.getActiveVoiceCount() == 1, "the stereo bus's voice is still playing");
	}

	// The active voice list is only recomputed by a mix, so a bus seek right
	// after a voice was stopped and another played in its slot mixed the new
	// voice, which has no resample buffers yet: getAudio wrote through a null
	// pointer.
	void bus_seek_skips_voices_without_buffers()
	{
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		Wav sound;
		check(loadConstantRaw(sound, 1, 30000, 0.25f) == SO_NO_ERROR, "raw wave loads");
		Bus bus;
		handle busHandle = soloud.play(bus);
		handle first = bus.play(sound, 1);
		std::vector<float> out(2 * 512);
		soloud.mix(out.data(), 512);
		soloud.stop(first);
		handle second = bus.play(sound, 1);
		check((second & 0xfff) == (first & 0xfff), "the new voice reuses the slot");
		check(soloud.seek(busHandle, 0.2) == SO_NO_ERROR, "bus seeks");
		soloud.mix(out.data(), 512);
		check(allFinite(out), "output stays finite");
		check(bus.getActiveVoiceCount() == 1, "the new voice still plays");
	}

	// Bus::getApproximateVolume compared the channel with `>` instead of
	// `>=`, so channel 8 of an 8-channel bus read past the volume array (into
	// the wave data).
	void bus_volume_channel_in_range()
	{
		Soloud soloud;
		check(initNull(soloud, 8), "null driver init");
		Wav sound;
		check(loadConstantRaw(sound, 8, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Bus bus;
		check(bus.setChannels(8) == SO_NO_ERROR, "8-channel bus");
		bus.setVisualizationEnable(true);
		soloud.play(bus);
		bus.play(sound, 1);
		std::vector<float> out(8 * 1024);
		soloud.mix(out.data(), 1024);
		check(bus.getWave()[0] != 0, "the bus has wave data");
		check(bus.getApproximateVolume(7) > 0, "channel 7 has a volume");
		check(bus.getApproximateVolume(8) == 0, "channel 8 does not exist");
	}

	// A looping sound in a queue returned no frames once at its end without
	// ever ending, so QueueInstance::getAudio looped forever inside the mix.
	void queue_looping_source_does_not_hang()
	{
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		Wav looping;
		check(loadConstantRaw(looping, 1, 1000, 0.25f) == SO_NO_ERROR, "raw wave loads");
		looping.setLooping(true);
		Wav next;
		check(loadConstantRaw(next, 1, 1000, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Queue queue;
		soloud.play(queue);
		check(queue.play(looping) == SO_NO_ERROR && queue.play(next) == SO_NO_ERROR, "sounds queue");
		std::vector<float> out(2 * 4096);
		soloud.mix(out.data(), 4096);
		check(queue.getQueueCount() == 0, "both sounds played");
	}

	// A queued sound with fewer channels than the queue left the other
	// channels of the voice's resample buffer as they were: two blocks back,
	// that was the previous sound's audio.
	void queue_silences_missing_channels()
	{
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		Wav stereo;
		check(loadConstantRaw(stereo, 2, 600, 0.25f) == SO_NO_ERROR, "raw wave loads");
		Wav mono;
		check(loadConstantRaw(mono, 1, 3000, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Queue queue;
		check(queue.setParams((float)kRate, 2) == SO_NO_ERROR, "stereo queue");
		soloud.play(queue);
		check(queue.play(stereo) == SO_NO_ERROR && queue.play(mono) == SO_NO_ERROR, "sounds queue");
		std::vector<float> out = render(soloud, 2048);
		check(maxAbsChannel(std::vector<float>(out.begin() + 2 * 700, out.end()), 1) == 0, "the mono sound plays on the left only");
	}

	// Fader::setLFO made the half range positive and centred the oscillation
	// on aFrom plus it, so oscillating from a higher value to a lower one ran
	// above aFrom: from 1 to 0 reached 2.
	void lfo_from_above_to_stays_in_range()
	{
		Fader down;
		down.setLFO(1.0f, 0.0f, 1.0, 0.0);
		Fader up;
		up.setLFO(0.0f, 1.0f, 1.0, 0.0);
		float low = 1.0f, high = 0.0f;
		for (int i = 0; i <= 200; i++)
		{
			double t = i / 100.0;
			float a = down.get(t);
			float b = up.get(t);
			low = a < low ? a : low;
			high = a > high ? a : high;
			check(std::fabs(a + b - 1.0f) < 1e-6f, "the two directions mirror each other");
		}
		std::printf("  1 -> 0 ranges over [%g, %g]\n", low, high);
		check(low > -1e-6f && high < 1.0f + 1e-6f, "the oscillation stays between to and from");
		check(std::fabs(down.get(0.0) - 0.5f) < 1e-6f, "it starts at the midpoint");
		check(std::fabs(down.get(0.25)) < 1e-6f, "and moves toward to first");
	}

	// #314: when a source block ran out, the resampler wrote one output frame
	// too few unless the block's remaining length was a whole number of steps,
	// and the next block started at its first frame, skipping the rest of the
	// step. A 44.1 kHz sound on a 48 kHz engine played about 0.09% fast (0.1 s
	// in 3 minutes, as reported). The playhead must follow the fixed-point
	// step exactly.
	void resampled_playhead_does_not_drift()
	{
		const unsigned int frames = 44100 * 20;
		std::vector<float> ramp(frames);
		for (unsigned int i = 0; i < frames; i++)
			ramp[i] = (float)i / (1 << 21);
		Wav wav;
		check(wav.loadRawWave(ramp.data(), frames, 44100.0f, 1, true) == SO_NO_ERROR, "raw wave loads");
		const unsigned int chunks[] = { 512, 441 };
		for (unsigned int chunk : chunks)
		{
			Soloud soloud;
			check(soloud.init(0, Soloud::NULLDRIVER, 48000, 512, 1) == SO_NO_ERROR, "null driver init");
			soloud.setMainResampler(Soloud::RESAMPLER_POINT);
			soloud.play(wav);
			const unsigned int outFrames = 48000 * 19;
			std::vector<float> out(outFrames, 0.0f);
			for (unsigned int f = 0; f < outFrames; f += chunk)
				soloud.mix(&out[f], outFrames - f < chunk ? outFrames - f : chunk);

			// The point resampler plays source frame (n * step) >> 20 at output
			// frame n; frame 100, in the first block, gives the gain of the pan law.
			const uint64_t step = (uint64_t)std::floor(44100.0f / 48000.0f * (1 << 20));
			const double gain = out[100] / (double)((100 * step) >> 20);
			long long worst = 0;
			unsigned int worstFrame = 0;
			for (unsigned int n = 100; n < outFrames; n++)
			{
				long long played = std::llround(out[n] / gain);
				long long expected = (long long)((n * step) >> 20);
				if (std::llabs(played - expected) > std::llabs(worst))
				{
					worst = played - expected;
					worstFrame = n;
				}
			}
			std::printf("  %u-frame mixes: worst playhead error %lld source frames at output frame %u\n", chunk, worst, worstFrame);
			check(worst == 0, "every output frame plays the source frame the step reaches");
		}
	}

	// The SSE 1->2 and 2->2 pan paths scaled the per-sample volume step by 4
	// for the quad loop and then reused it, from the starting volume, for the
	// frames left over when a mix call is not a multiple of 4 frames long.
	void pan_ramp_leftover_frames()
	{
		const float target = (float)std::cos(M_PI / 4);
		for (unsigned int channels = 1; channels <= 2; channels++)
		{
			Wav wav;
			check(loadConstantRaw(wav, channels, 4096, 1.0f) == SO_NO_ERROR, "raw wave loads");
			Soloud soloud;
			check(initNull(soloud), "null driver init");
			soloud.setPostClipScaler(1.0f);
			handle h = soloud.play(wav, 0.0f);
			soloud.setVolume(h, 1.0f);
			float out[6 * 2];
			soloud.mix(out, 6);
			// Linear resampling delays the source by one frame, so frame 0 is
			// silent; frame j then carries the ramp value target * (j + 1) / 6.
			for (unsigned int j = 1; j < 6; j++)
			{
				float expected = target * (float)(j + 1) / 6.0f;
				std::printf("  %u->2 frame %u: %g (expected %g)\n", channels, j, out[j * 2], expected);
				check(std::fabs(out[j * 2] - expected) < 1e-5f, "left channel follows the volume ramp");
				check(std::fabs(out[j * 2 + 1] - expected) < 1e-5f, "right channel follows the volume ramp");
			}
		}
	}

	// 6->4 and 8->4 added the center and LFE channels without the voice's
	// volume, so a silent voice was still audible.
	void surround_to_quad_center_follows_volume()
	{
		for (unsigned int channels = 6; channels <= 8; channels += 2)
		{
			Wav wav;
			check(loadConstantRaw(wav, channels, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
			Soloud soloud;
			check(initNull(soloud, 4), "null driver init");
			soloud.play(wav, 0.0f);
			std::vector<float> out = render(soloud, 1024, 4);
			for (unsigned int c = 0; c < 4; c++)
			{
				std::printf("  %u->4 channel %u peak %g\n", channels, c, maxAbsChannel(out, c, 4));
				check(maxAbsChannel(out, c, 4) == 0.0f, "a voice at volume 0 is silent");
			}
		}
	}

	// 4->8 scaled the rear pair by the side speakers' volumes.
	void quad_to_7_1_rear_uses_rear_volume()
	{
		Wav wav;
		check(loadConstantRaw(wav, 4, 4096, 0.25f) == SO_NO_ERROR, "raw wave loads");
		Soloud soloud;
		check(initNull(soloud, 8), "null driver init");
		soloud.setPostClipScaler(1.0f);
		soloud.play3d(wav, 0.0f, 0.0f, 10.0f);
		soloud.update3dAudio();
		std::vector<float> out = render(soloud, 2048, 8);
		const float *last = &out[out.size() - 8];
		std::printf("  side %g %g, rear %g %g\n", last[4], last[5], last[6], last[7]);
		// The side speakers sit at z = 0 and the rear ones at z = -1, so a
		// source on the z axis reaches them with different gains.
		check(std::fabs(last[6] - last[4]) > 0.01f, "left rear differs from left side");
		check(std::fabs(last[7] - last[5]) > 0.01f, "right rear differs from right side");
	}

	// When every voice was protected, findFreeVoice_internal stopped voice -1
	// and play returned UNKNOWN_ERROR as if it were a handle.
	void play_without_free_voice()
	{
		Wav wav;
		check(loadConstantRaw(wav, 1, 4096, 0.0f) == SO_NO_ERROR, "raw wave loads");
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		for (int i = 0; i < VOICE_COUNT; i++)
			soloud.setProtectVoice(soloud.play(wav), true);
		handle h = soloud.play(wav);
		std::printf("  handle with no free voice: %u\n", h);
		check(h == 0, "play returns the invalid handle 0");
		check(soloud.getVoiceCount() == VOICE_COUNT, "no protected voice was stopped");
	}

	// Handles whose slot bits are 0 or past VOICE_COUNT indexed mVoice out of
	// bounds.
	void handle_slot_out_of_range()
	{
		Wav wav;
		check(loadConstantRaw(wav, 1, 4096, 0.0f) == SO_NO_ERROR, "raw wave loads");
		Soloud soloud;
		check(initNull(soloud), "null driver init");
		soloud.play(wav);
		const handle bad[] = { 1u << 12, (1u << 12) | 0xfff, (1u << 12) | (VOICE_COUNT + 1) };
		for (handle h : bad)
		{
			check(!soloud.isValidVoiceHandle(h), "out-of-range slot is not a valid handle");
			check(soloud.getVolume(h) == 0.0f, "getter on an out-of-range slot returns the default");
			soloud.stop(h);
		}
		check(soloud.getVoiceCount() == 1, "the playing voice is untouched");
	}

	// loadRawWave* accepted more than MAX_CHANNELS channels (overflowing the
	// resample buffers) and data shorter than one frame (zero frames).
	void raw_wave_rejects_bad_layout()
	{
		std::vector<float> f(64, 0.0f);
		std::vector<short> s(64, 0);
		std::vector<unsigned char> b(64, 128);
		Wav wav;
		check(wav.loadRawWave(f.data(), 18, (float)kRate, MAX_CHANNELS + 1, true, false) == INVALID_PARAMETER, "float: 9 channels rejected");
		check(wav.loadRawWave(f.data(), 1, (float)kRate, 2, true, false) == INVALID_PARAMETER, "float: less than a frame rejected");
		check(wav.loadRawWave16(s.data(), 18, (float)kRate, MAX_CHANNELS + 1) == INVALID_PARAMETER, "16-bit: 9 channels rejected");
		check(wav.loadRawWave16(s.data(), 1, (float)kRate, 2) == INVALID_PARAMETER, "16-bit: less than a frame rejected");
		check(wav.loadRawWave8(b.data(), 18, (float)kRate, MAX_CHANNELS + 1) == INVALID_PARAMETER, "8-bit: 9 channels rejected");
		check(wav.loadRawWave8(b.data(), 1, (float)kRate, 2) == INVALID_PARAMETER, "8-bit: less than a frame rejected");
		check(wav.loadRawWave(f.data(), 16, (float)kRate, MAX_CHANNELS, true, false) == SO_NO_ERROR, "float: 8 channels accepted");
	}

	// An engine whose memory is poisoned once it is destroyed, the way a debug
	// heap marks freed memory, so a later read through a stale pointer fails
	// instead of finding the old values.
	struct PoisonedEngine
	{
		void *mMemory;
		Soloud *mEngine;

		PoisonedEngine() : mMemory(std::malloc(sizeof(Soloud))), mEngine(new (mMemory) Soloud) {}

		void destroy()
		{
			mEngine->~Soloud();
			std::memset(mMemory, 0xdd, sizeof(Soloud));
		}

		~PoisonedEngine() { std::free(mMemory); }
	};

	// AudioSource::mSoloud kept pointing at a destroyed engine, so stopping or
	// destroying a sound afterwards used freed memory (it hung on the freed
	// mutex).
	void source_outlives_its_engine()
	{
		Wav *wav = new Wav;
		check(loadConstantRaw(*wav, 1, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		PoisonedEngine dead;
		check(initNull(*dead.mEngine), "null driver init");
		dead.mEngine->play(*wav);
		dead.destroy();
		wav->stop();
		delete wav;
	}

	// A sound played on two engines was stopped only on the engine it was last
	// played on, so destroying it left the other voice reading freed samples.
	void source_stops_on_every_engine()
	{
		Wav *wav = new Wav;
		check(loadConstantRaw(*wav, 1, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Soloud a, b;
		check(initNull(a) && initNull(b), "null driver init");
		a.play(*wav);
		b.play(*wav);
		wav->stop();
		check(a.getVoiceCount() == 0, "stop() stops the voice on the first engine");
		check(b.getVoiceCount() == 0, "stop() stops the voice on the second engine");
		a.play(*wav);
		b.play(*wav);
		delete wav;
		check(a.getVoiceCount() == 0, "destroying the sound stops it on the first engine");
		check(b.getVoiceCount() == 0, "destroying the sound stops it on the second engine");
		render(a, 1024);
		render(b, 1024);
	}

	// Sound IDs were counted per engine, so sounds first played on different
	// engines could share an ID, and stopping one stopped the other.
	void source_ids_unique_across_engines()
	{
		Wav x, y;
		check(loadConstantRaw(x, 1, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		check(loadConstantRaw(y, 1, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Soloud a, b;
		check(initNull(a) && initNull(b), "null driver init");
		a.play(x);
		b.play(y);
		b.play(x);
		check(b.countAudioSource(y) == 1, "the second sound counts only its own voice");
		x.stop();
		check(b.countAudioSource(y) == 1, "stopping the first sound leaves the second playing");
		check(a.getVoiceCount() == 0 && b.getVoiceCount() == 1, "only the first sound's voices stopped");
	}

	// Bus kept its instance pointer after the instance was destroyed, so a bus
	// used after its engine was destroyed read freed memory.
	void bus_outlives_its_engine()
	{
		Wav wav;
		check(loadConstantRaw(wav, 1, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Bus *bus = new Bus;
		PoisonedEngine dead;
		check(initNull(*dead.mEngine), "null driver init");
		dead.mEngine->play(*bus);
		check(bus->play(wav) != 0, "the bus plays a sound");
		dead.destroy();
		check(bus->play(wav) == 0, "a bus whose engine is gone does not play");
		check(bus->getActiveVoiceCount() == 0, "a bus whose engine is gone has no voices");
		bus->annexSound(1);
		bus->setFilter(0, 0);
		bus->getWave();
		bus->calcFFT();
		check(bus->getApproximateVolume(0) == 0.0f, "a bus whose engine is gone is silent");
		delete bus;
	}

	// Queue had the same stale instance and engine pointers as Bus.
	void queue_outlives_its_engine()
	{
		Wav wav;
		check(loadConstantRaw(wav, 1, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Queue *queue = new Queue;
		queue->setParamsFromAudioSource(wav);
		PoisonedEngine dead;
		check(initNull(*dead.mEngine), "null driver init");
		dead.mEngine->play(*queue);
		check(queue->play(wav) == SO_NO_ERROR, "the queue accepts a sound");
		dead.destroy();
		check(queue->play(wav) != SO_NO_ERROR, "a queue whose engine is gone accepts nothing");
		check(queue->getQueueCount() == 1, "the unplayed sound is still counted");
		check(!queue->isCurrentlyPlaying(wav), "a queue whose engine is gone plays nothing");
		delete queue;
	}

	// Playing a bus on a second engine left its old instance on the first, and
	// destroying that instance stopped the bus's voices on the second engine.
	void bus_moved_to_another_engine()
	{
		Wav wav;
		check(loadConstantRaw(wav, 1, 4096, 0.5f) == SO_NO_ERROR, "raw wave loads");
		Bus bus;
		Soloud *a = new Soloud;
		Soloud b;
		check(initNull(*a) && initNull(b), "null driver init");
		a->play(bus);
		b.play(bus);
		check(a->getVoiceCount() == 0, "the bus left the first engine");
		check(bus.play(wav) != 0, "the bus plays on the second engine");
		delete a;
		check(b.getVoiceCount() == 2, "destroying the first engine leaves the second engine's voices");
	}

	// Sounds destroyed on one thread stop on every engine while another thread
	// creates, initializes and destroys engines.
	void sources_and_engines_on_two_threads()
	{
		Soloud shared;
		check(initNull(shared), "null driver init");
		std::thread engines([] {
			Wav wav;
			loadConstantRaw(wav, 1, 4096, 0.5f);
			for (int i = 0; i < 500; i++)
			{
				Soloud engine;
				initNull(engine);
				engine.play(wav);
			}
		});
		for (int i = 0; i < 2000; i++)
		{
			Wav *wav = new Wav;
			loadConstantRaw(*wav, 1, 64, 0.5f);
			shared.play(*wav);
			delete wav;
		}
		engines.join();
		check(shared.getVoiceCount() == 0, "every destroyed sound was stopped");
	}

	// Ogg Vorbis streams kept the decoded Vorbis frame they were reading
	// across a rewind or seek. stb_vorbis reuses its buffers, so a looping
	// stream played the rest of that frame, now holding other audio, before
	// the start; a seek also landed after the frame holding its target.
	struct OggFixture
	{
		Wav wav;
		WavStream stream;
		bool ok;
		OggFixture(const char *aName)
		{
			std::string path = std::string(SOLOUDER_FIXTURE_DIR) + "/ogg/" + aName;
			ok = wav.load(path.c_str()) == SO_NO_ERROR && stream.load(path.c_str()) == SO_NO_ERROR;
			check(ok, "Ogg fixture loads as Wav and WavStream");
		}
	};

	// Reads aFrames of channel 0 in calls of at most aCall frames.
	std::vector<float> readChannel0(AudioSourceInstance *aInstance, unsigned int aFrames, unsigned int aCall)
	{
		std::vector<float> out;
		std::vector<float> buffer(aCall * MAX_CHANNELS);
		while (out.size() < aFrames)
		{
			unsigned int n = aFrames - (unsigned int)out.size() < aCall ? aFrames - (unsigned int)out.size() : aCall;
			unsigned int got = aInstance->getAudio(buffer.data(), n, aCall);
			out.insert(out.end(), buffer.begin(), buffer.begin() + got);
			if (got < n)
				break;
		}
		return out;
	}

	unsigned int countMismatches(const std::vector<float> &aGot, const Wav &aWav, unsigned int aFrom)
	{
		unsigned int wrong = 0;
		for (size_t i = 0; i < aGot.size(); i++)
		{
			if (aFrom + i >= aWav.mSampleCount || aGot[i] != aWav.mData[aFrom + i])
				wrong++;
		}
		return wrong;
	}

	void oggstream_rewind_restarts()
	{
		OggFixture f("vorbis_stereo_44100.ogg");
		if (!f.ok)
			return;
		AudioSourceInstance *instance = f.stream.createInstance();
		instance->init(f.stream, 0);
		check(countMismatches(readChannel0(instance, 1000, 300), f.wav, 0) == 0, "first frames match the Wav");
		instance->rewind();
		std::vector<float> again = readChannel0(instance, 2048, 512);
		check(again.size() == 2048, "2048 frames after the rewind");
		unsigned int wrong = countMismatches(again, f.wav, 0);
		if (wrong)
			std::printf("  %u of 2048 frames differ after the rewind\n", wrong);
		check(wrong == 0, "a rewound stream starts over");
		delete instance;
	}

	void oggstream_seek_lands_on_target()
	{
		OggFixture f("vorbis_stereo_44100.ogg");
		if (!f.ok)
			return;
		AudioSourceInstance *instance = f.stream.createInstance();
		instance->init(f.stream, 0);
		std::vector<float> scratch(512 * MAX_CHANNELS);
		// Forward from the start: frame floor(44100 * 0.5) = 22050.
		instance->seek(0.5, scratch.data(), (unsigned int)scratch.size());
		std::vector<float> got = readChannel0(instance, 1500, 512);
		unsigned int wrong = countMismatches(got, f.wav, 22050);
		if (wrong)
			std::printf("  forward seek: %u of %u frames differ\n", wrong, (unsigned int)got.size());
		check(got.size() == 1500 && wrong == 0, "a forward seek plays from its target");
		// Backward: rewinds, then frame 11025.
		instance->seek(0.25, scratch.data(), (unsigned int)scratch.size());
		got = readChannel0(instance, 700, 300);
		wrong = countMismatches(got, f.wav, 11025);
		if (wrong)
			std::printf("  backward seek: %u of %u frames differ\n", wrong, (unsigned int)got.size());
		check(got.size() == 700 && wrong == 0, "a backward seek plays from its target");
		// Forward again counts from the frames read since: 11025 + 700, plus
		// floor(44100 * 0.25) for the quarter second from 0.25 to 0.5.
		instance->seek(0.5, scratch.data(), (unsigned int)scratch.size());
		got = readChannel0(instance, 1000, 512);
		wrong = countMismatches(got, f.wav, 11025 + 700 + 11025);
		if (wrong)
			std::printf("  relative seek: %u of %u frames differ\n", wrong, (unsigned int)got.size());
		check(got.size() == 1000 && wrong == 0, "a seek counts from the frames read, as the generic seek does");
		check(!instance->hasEnded(), "not ended");
		delete instance;
	}

	void oggstream_seek_past_end_ends()
	{
		OggFixture f("vorbis_stereo_44100.ogg");
		if (!f.ok)
			return;
		AudioSourceInstance *instance = f.stream.createInstance();
		instance->init(f.stream, 0);
		std::vector<float> scratch(512 * MAX_CHANNELS);
		readChannel0(instance, 333, 333);
		instance->seek(10.0, scratch.data(), (unsigned int)scratch.size());
		check(instance->hasEnded(), "a seek past the end ends the stream");
		std::vector<float> buffer(512 * MAX_CHANNELS, 1.0f);
		check(instance->getAudio(buffer.data(), 512, 512) == 0, "nothing more is read");
		// The last frames, from a seek just before the end.
		instance->rewind();
		instance->seek(1.49, scratch.data(), (unsigned int)scratch.size());
		unsigned int from = (unsigned int)std::floor(44100 * 1.49);
		std::vector<float> got = readChannel0(instance, 2000, 512);
		check(got.size() == f.wav.mSampleCount - from, "the frames to the end, and no more");
		check(countMismatches(got, f.wav, from) == 0, "the last frames match the Wav");
		check(instance->hasEnded(), "ended at the end");
		delete instance;
	}

	// stb_vorbis reads an Ogg page with no segments through SoLoud's File
	// as fread(data, 0, 1), and Soloud_Filehack_fread divided the bytes
	// read by the element size of 0: an Ogg file cut inside its first page
	// crashed WavStream (integer division by zero).
	void oggstream_cut_first_page_fails()
	{
		std::string path = std::string(SOLOUDER_FIXTURE_DIR) + "/ogg/bad_cut_header.ogg";
		WavStream fromFile;
		check(fromFile.load(path.c_str()) == FILE_LOAD_FAILED, "load fails");
		WavStream inMemory;
		check(inMemory.loadToMem(path.c_str()) == FILE_LOAD_FAILED, "loadToMem fails");
	}

	// dr_flac 0.12.35 (upstream 713ffe6): the LPC sum was taken in 32 bits
	// whenever bitsPerSample + shift <= 32, ignoring the coefficient precision
	// and order. A predictor that overshoots full scale wrapped around. This
	// file has one 24-bit frame with an order-8 LPC subframe (precision 11,
	// shift 8) whose samples are round(8000000 * sin(2 pi i / 23)).
	void flac_lpc_prediction_uses_64_bits()
	{
		static const unsigned char kFlac[] = {
			0x66, 0x4c, 0x61, 0x43, 0x80, 0x00, 0x00, 0x22, 0x00, 0x40, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00,
			0x00, 0x00, 0x0a, 0xc4, 0x41, 0x70, 0x00, 0x00, 0x00, 0x40, 0x80, 0x4d, 0xdb, 0xe5, 0x37, 0xa7,
			0x20, 0x45, 0x89, 0x3b, 0xd2, 0xfc, 0xbc, 0x71, 0xe1, 0x6d, 0xff, 0xf8, 0x60, 0x00, 0x00, 0x3f,
			0x57, 0x4e, 0x00, 0x00, 0x00, 0x20, 0xef, 0x26, 0x3f, 0x6d, 0x00, 0x59, 0x36, 0xa0, 0x6c, 0x62,
			0x6a, 0x77, 0x84, 0x61, 0x79, 0xc9, 0x26, 0x73, 0x05, 0xa7, 0xa4, 0x2e, 0xec, 0x4c, 0x2b, 0xbe,
			0x70, 0x18, 0xfe, 0x40, 0x23, 0xff, 0x21, 0x62, 0xdb, 0xf3, 0x29, 0x24, 0xd3, 0xfd, 0xb2, 0xfa,
			0x2d, 0x20, 0xdd, 0xa6, 0xf2, 0xa9, 0x3c, 0xdc, 0xcf, 0xaf, 0x1d, 0x24, 0x0b, 0xf9, 0x97, 0x4f,
			0xf2, 0xae, 0x05, 0x5e, 0x7c, 0xe3, 0x08, 0xe0, 0x31, 0x81, 0x75, 0x70, 0xa7, 0x73, 0xe4, 0xf8,
			0x43, 0x84, 0xb6, 0xa9, 0x85, 0xcf, 0x06, 0xca, 0x42, 0xbf, 0xf6, 0x3b, 0x02, 0x14, 0xd1, 0x2b,
			0x17, 0x0b, 0xde, 0x32, 0x60, 0x64, 0x69, 0x73, 0xe0, 0xc4, 0xc4, 0x7f, 0x12, 0x5b, 0x7e, 0x65,
			0x24, 0x9a, 0x7f, 0xb6, 0x5f, 0x45, 0xa4, 0x1b, 0xb4, 0xde, 0x55, 0x27, 0x9b, 0x99, 0xf5, 0xe3,
			0xa4, 0x81, 0x7f, 0x32, 0xe9, 0xfe, 0x55, 0xc0, 0xab, 0xcf, 0x9c, 0x61, 0x1c, 0x06, 0x30, 0x2e,
			0xae, 0x14, 0xee, 0x7c, 0x9f, 0x08, 0x70, 0x96, 0xd5, 0x30, 0xb9, 0xe0, 0xd9, 0x48, 0x57, 0xfe,
			0xc7, 0x60, 0x42, 0x9a, 0x25, 0x62, 0xe1, 0x7b, 0xc6, 0x4c, 0x0c, 0x8d, 0x2e, 0x7c, 0x18, 0x98,
			0x8f, 0xe2, 0x4b, 0x6f, 0xcc, 0xa4, 0x93, 0x4f, 0xf6, 0xcb, 0xe8, 0xb4, 0x83, 0x76, 0x9b, 0xca,
			0xa4, 0xf3, 0x73, 0x3e, 0xbc, 0x74, 0x90, 0x2f, 0xe6, 0x5d, 0x3f, 0xca, 0xb8, 0x15, 0x79, 0xf3,
			0x80, 0xc7, 0x8f
		};
		Wav wav;
		check(wav.loadMem(kFlac, sizeof(kFlac), true, false) == SO_NO_ERROR, "load");
		check(wav.mSampleCount == 64, "64 frames");
		if (wav.mSampleCount != 64)
			return;
		int wrong = 0;
		for (int i = 0; i < 64; i++)
		{
			double expected = std::round(8000000.0 * std::sin(2.0 * 3.14159265358979323846 * i / 23.0));
			if (std::fabs(wav.mData[i] * 8388608.0 - expected) > 1.0)
				wrong++;
		}
		check(wrong == 0, "every sample decodes");
	}

	struct TestCase
	{
		const char *mName;
		void (*mRun)();
	};

	const TestCase kTests[] = {
		{ "wavstream_loop_channel_stride", wavstream_loop_channel_stride },
		{ "wavstream_looping_not_ended", wavstream_looping_not_ended },
		{ "wavstream_truncated_data_ends", wavstream_truncated_data_ends },
		{ "wavstream_missing_file_ends", wavstream_missing_file_ends },
		{ "inaudible_tick_over_active_cap", inaudible_tick_over_active_cap },
		{ "protected_voice_stays_active", protected_voice_stays_active },
		{ "protecting_a_voice_makes_it_active", protecting_a_voice_makes_it_active },
		{ "ticking_a_voice_makes_it_active", ticking_a_voice_makes_it_active },
		{ "sound_tail_survives_unaligned_mix_calls", sound_tail_survives_unaligned_mix_calls },
		{ "zero_step_voice_no_stale_scratch", zero_step_voice_no_stale_scratch },
		{ "extreme_speed_makes_progress", extreme_speed_makes_progress },
		{ "generic_seek_small_scratch", generic_seek_small_scratch },
		{ "negative_sample_rate_renders_silence", negative_sample_rate_renders_silence },
		{ "seek_ignores_play_speed", seek_ignores_play_speed },
		{ "loop_point_keeps_earlier_frames", loop_point_keeps_earlier_frames },
		{ "hard_pan_is_silent_on_the_far_side", hard_pan_is_silent_on_the_far_side },
		{ "biquad_above_voice_nyquist_stays_finite", biquad_above_voice_nyquist_stays_finite },
		{ "lofi_filters_eight_channels", lofi_filters_eight_channels },
		{ "echo_with_zero_delay_survives", echo_with_zero_delay_survives },
		{ "waveshaper_minimum_amount_is_finite", waveshaper_minimum_amount_is_finite },
		{ "dc_removal_outlives_its_filter", dc_removal_outlives_its_filter },
		{ "dc_removal_long_length_is_safe", dc_removal_long_length_is_safe },
		{ "filter_params_reject_nan", filter_params_reject_nan },
		{ "biquad_single_sample_stays_in_bounds", biquad_single_sample_stays_in_bounds },
		{ "freeverb_leaves_mono_unchanged", freeverb_leaves_mono_unchanged },
		{ "freeverb_uses_its_settings", freeverb_uses_its_settings },
		{ "freeverb_and_robotize_fade_parameters", freeverb_and_robotize_fade_parameters },
		{ "robotize_period_and_phase_are_safe", robotize_period_and_phase_are_safe },
		{ "flanger_channels_share_the_lfo", flanger_channels_share_the_lfo },
		{ "flanger_delay_extremes_are_safe", flanger_delay_extremes_are_safe },
		{ "more_filter_params_reject_nan", more_filter_params_reject_nan },
		{ "bus_seek_mixes_in_blocks", bus_seek_mixes_in_blocks },
		{ "bus_volume_channel_in_range", bus_volume_channel_in_range },
		{ "bus_seek_skips_voices_without_buffers", bus_seek_skips_voices_without_buffers },
		{ "queue_looping_source_does_not_hang", queue_looping_source_does_not_hang },
		{ "queue_silences_missing_channels", queue_silences_missing_channels },
		{ "lfo_from_above_to_stays_in_range", lfo_from_above_to_stays_in_range },
		{ "resampled_playhead_does_not_drift", resampled_playhead_does_not_drift },
		{ "play_stop_without_init", play_stop_without_init },
		{ "play_after_failed_init", play_after_failed_init },
		{ "wav_rejects_too_many_channels", wav_rejects_too_many_channels },
		{ "wavstream_rejects_too_many_channels", wavstream_rejects_too_many_channels },
		{ "active_voice_cap_above_255", active_voice_cap_above_255 },
		{ "play3d_reused_slot_flags", play3d_reused_slot_flags },
		{ "inaudible_tick_empty_loop_ends", inaudible_tick_empty_loop_ends },
		{ "inaudible_tick_clears_short_block", inaudible_tick_clears_short_block },
		{ "pan_ramp_leftover_frames", pan_ramp_leftover_frames },
		{ "surround_to_quad_center_follows_volume", surround_to_quad_center_follows_volume },
		{ "quad_to_7_1_rear_uses_rear_volume", quad_to_7_1_rear_uses_rear_volume },
		{ "play_without_free_voice", play_without_free_voice },
		{ "handle_slot_out_of_range", handle_slot_out_of_range },
		{ "raw_wave_rejects_bad_layout", raw_wave_rejects_bad_layout },
		{ "source_outlives_its_engine", source_outlives_its_engine },
		{ "source_stops_on_every_engine", source_stops_on_every_engine },
		{ "source_ids_unique_across_engines", source_ids_unique_across_engines },
		{ "bus_outlives_its_engine", bus_outlives_its_engine },
		{ "queue_outlives_its_engine", queue_outlives_its_engine },
		{ "bus_moved_to_another_engine", bus_moved_to_another_engine },
		{ "sources_and_engines_on_two_threads", sources_and_engines_on_two_threads },
		{ "flac_lpc_prediction_uses_64_bits", flac_lpc_prediction_uses_64_bits },
		{ "oggstream_rewind_restarts", oggstream_rewind_restarts },
		{ "oggstream_seek_lands_on_target", oggstream_seek_lands_on_target },
		{ "oggstream_seek_past_end_ends", oggstream_seek_past_end_ends },
		{ "oggstream_cut_first_page_fails", oggstream_cut_first_page_fails },
		{ "noise_zero_octave_sum_is_silent", noise_zero_octave_sum_is_silent },
		{ "wavstream_reload_stops_voices", wavstream_reload_stops_voices },
		{ "vic_register_index_in_range", vic_register_index_in_range },
		{ "sfxr_short_file_fails", sfxr_short_file_fails },
		{ "sfxr_filter_flag_is_a_bool", sfxr_filter_flag_is_a_bool },
		{ "sfxr_ended_sound_stays_silent", sfxr_ended_sound_stays_silent },
		{ "monotone_empty_song_is_silent", monotone_empty_song_is_silent },
		{ "monotone_short_file_fails", monotone_short_file_fails },
		{ "monotone_long_title_is_cut", monotone_long_title_is_cut },
		{ "monotone_at_most_12_channels", monotone_at_most_12_channels },
		{ "monotone_indices_in_range", monotone_indices_in_range },
		{ "monotone_note_table_is_filled", monotone_note_table_is_filled },
		{ "monotone_slow_rate_does_not_divide_by_zero", monotone_slow_rate_does_not_divide_by_zero },
		{ "monotone_first_slide_starts_at_zero", monotone_first_slide_starts_at_zero },
		{ "tedsid_model_from_dump", tedsid_model_from_dump },
		{ "tedsid_reload_stops_voices", tedsid_reload_stops_voices },
		{ "tedsid_header_is_checked", tedsid_header_is_checked },
		{ "tedsid_loop_without_wait_fails", tedsid_loop_without_wait_fails },
		{ "tedsid_zx7_stays_in_bounds", tedsid_zx7_stays_in_bounds },
		{ "sid_envelope_rate_starts_in_range", sid_envelope_rate_starts_in_range },
		{ "sid_attack_from_full_level_wraps", sid_attack_from_full_level_wraps },
		{ "ay_right_channel_at_buffer_stride", ay_right_channel_at_buffer_stride },
		{ "ay_reload_stops_voices", ay_reload_stops_voices },
		{ "ay_header_is_checked", ay_header_is_checked },
		{ "ay_zx7_stays_in_bounds", ay_zx7_stays_in_bounds },
		{ "ay_speeds_are_checked", ay_speeds_are_checked },
		{ "ay_chip_starts_at_zero", ay_chip_starts_at_zero },
		{ "ay_last_write_is_played", ay_last_write_is_played },
		{ "ay_long_wait_is_spread", ay_long_wait_is_spread },
		{ "ay_tail_is_played", ay_tail_is_played },
		{ "ay_rewind_restarts", ay_rewind_restarts },
	};
}

int main(int argc, char **argv)
{
	if (argc != 2)
	{
		std::printf("usage: reference_regressions <test-name|all>\n");
		return 2;
	}
	std::string wanted = argv[1];
	bool found = false;
	for (const TestCase &test : kTests)
	{
		if (wanted != "all" && wanted != test.mName)
			continue;
		found = true;
		std::printf("%s\n", test.mName);
		std::fflush(stdout);
		int before = gFailures;
		test.mRun();
		std::printf("  %s\n", gFailures == before ? "ok" : "FAILED");
		std::fflush(stdout);
	}
	if (!found)
	{
		std::printf("unknown test: %s\n", argv[1]);
		return 2;
	}
	return gFailures == 0 ? 0 : 1;
}

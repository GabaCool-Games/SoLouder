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
#include "soloud_bassboostfilter.h"
#include "soloud_biquadresonantfilter.h"
#include "soloud_bus.h"
#include "soloud_dcremovalfilter.h"
#include "soloud_echofilter.h"
#include "soloud_flangerfilter.h"
#include "soloud_freeverbfilter.h"
#include "soloud_lofifilter.h"
#include "soloud_robotizefilter.h"
#include "soloud_waveshaperfilter.h"
#include "soloud_queue.h"
#include "soloud_wav.h"
#include "soloud_wavstream.h"

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

	// Planar raw samples, every channel set to aValue.
	result loadConstantRaw(Wav &aWav, unsigned int aChannels, unsigned int aFrames, float aValue)
	{
		std::vector<float> data(aChannels * aFrames, aValue);
		return aWav.loadRawWave(data.data(), (unsigned int)data.size(), (float)kRate, aChannels, true, false);
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

	struct TestCase
	{
		const char *mName;
		void (*mRun)();
	};

	const TestCase kTests[] = {
		{ "wavstream_loop_channel_stride", wavstream_loop_channel_stride },
		{ "wavstream_looping_not_ended", wavstream_looping_not_ended },
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
		{ "play_stop_without_init", play_stop_without_init },
		{ "play_after_failed_init", play_after_failed_init },
		{ "wav_rejects_too_many_channels", wav_rejects_too_many_channels },
		{ "wavstream_rejects_too_many_channels", wavstream_rejects_too_many_channels },
		{ "active_voice_cap_above_255", active_voice_cap_above_255 },
		{ "play3d_reused_slot_flags", play3d_reused_slot_flags },
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

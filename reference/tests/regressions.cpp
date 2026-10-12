// Regression tests for defects fixed in the patched SoLoud C++ reference.
// Each test is run in its own process (see CMakeLists.txt); a crash, hang, or
// nonzero exit fails only that test. Upstream references are in the test names'
// comments and in docs/PORTING.md.

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

#include "soloud.h"
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
		{ "zero_step_voice_no_stale_scratch", zero_step_voice_no_stale_scratch },
		{ "generic_seek_small_scratch", generic_seek_small_scratch },
		{ "play_stop_without_init", play_stop_without_init },
		{ "play_after_failed_init", play_after_failed_init },
		{ "wav_rejects_too_many_channels", wav_rejects_too_many_channels },
		{ "wavstream_rejects_too_many_channels", wavstream_rejects_too_many_channels },
		{ "active_voice_cap_above_255", active_voice_cap_above_255 },
		{ "play3d_reused_slot_flags", play3d_reused_slot_flags },
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

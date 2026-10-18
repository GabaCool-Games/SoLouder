// Scenario operations on generators (SoLoud's audio sources other than Wav,
// WavStream, Bus and Queue). Included inside parity_runner.cpp's Runner;
// keep identical to crates/solouder-parity/src/generator_ops.rs.

		// The AudioSource settings every generator operation takes.
		static bool isSourceSettingKey(const std::string &key)
		{
			return key == "volume" || key == "looping" || key == "loop_point" || key == "auto_stop" || key == "single" ||
				key == "tick" || key == "kill";
		}

		// checkKeys with the source settings allowed too.
		void checkGeneratorKeys(const Step &step, const char *const *allowed, size_t count)
		{
			for (size_t i = 0; i < step.args.size(); i++)
			{
				const std::string &key = step.args[i].first;
				bool known = isSourceSettingKey(key);
				for (size_t k = 0; k < count; k++)
					known = known || key == allowed[k];
				if (!known)
					throw ScenarioError(step.line, "unknown argument `" + key + "` for `" + step.op + "`");
			}
		}

		// setVolume, setLooping, setLoopPoint, setAutoStop, setSingleInstance
		// and setInaudibleBehavior, at their defaults when not given.
		void sourceSettings(const Step &step, SoLoud::AudioSource &source)
		{
			source.setVolume(floatArg(step, "volume", 1.0f));
			source.setLooping(flagArg(step, "looping", false));
			source.setLoopPoint(timeArg(step, "loop_point", 0));
			source.setAutoStop(flagArg(step, "auto_stop", true));
			source.setSingleInstance(flagArg(step, "single", false));
			source.setInaudibleBehavior(flagArg(step, "tick", false), flagArg(step, "kill", false));
		}

		// Registers a generator; one under an existing name is deleted first,
		// which stops its voices.
		void storeGenerator(const std::string &name, SoLoud::AudioSource *source)
		{
			std::map<std::string, SoLoud::AudioSource *>::iterator old = generators.find(name);
			if (old != generators.end())
				delete old->second;
			generators[name] = source;
		}

		template <class T> T &generatorArg(const Step &step, const char *key, const char *what)
		{
			const std::string &name = textArg(step, key);
			std::map<std::string, SoLoud::AudioSource *>::iterator it = generators.find(name);
			T *source = it == generators.end() ? 0 : dynamic_cast<T *>(it->second);
			if (!source)
				throw ScenarioError(step.line, std::string("no ") + what + " named `" + name + "`");
			return *source;
		}

		// `s0` to `s9` for Noise::setOctaveScale, missing ones 0; true if any
		// is given.
		bool octaveScale(const Step &step, float *aScale)
		{
			bool any = false;
			for (int i = 0; i < 10; i++)
			{
				char key[4] = { 's', (char)('0' + i), 0, 0 };
				any = any || step.get(key) != 0;
				aScale[i] = floatArg(step, key, 0.0f);
			}
			return any;
		}

		void applyNoise(const Step &step, SoLoud::Noise &noise)
		{
			if (step.get("type"))
				noise.setType((int)uintArg(step, "type", true, 0));
			float s[10];
			if (octaveScale(step, s))
				noise.setOctaveScale(s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8], s[9]);
		}

		// `value` keys of 0 to 255.
		unsigned char byteArg(const Step &step, const char *key)
		{
			unsigned int value = uintArg(step, key, true, 0);
			if (value > 255)
				throw ScenarioError(step.line, std::string("`") + key + "` must be 0 to 255");
			return (unsigned char)value;
		}

		// `model` (0 PAL or 1 NTSC), then `bass`, `alto`, `soprano`, `noise`
		// and `reg` with `value`, each as given.
		void applyVic(const Step &step, SoLoud::Vic &vic)
		{
			if (step.get("model"))
			{
				unsigned int model = uintArg(step, "model", true, 0);
				if (model > 1)
					throw ScenarioError(step.line, "`model` must be 0 or 1");
				vic.setModel((int)model);
			}
			static const char *const regs[] = { "bass", "alto", "soprano", "noise" };
			for (int i = 0; i < 4; i++)
				if (step.get(regs[i]))
					vic.setRegister(i, byteArg(step, regs[i]));
			static const char *const pair[] = { "reg", "value" };
			if (groupArg(step, pair, 2, "`reg` and `value`"))
				vic.setRegister((int)uintArg(step, "reg", true, 0), byteArg(step, "value"));
		}

		// `preset` (with `seed`, default 0) for Sfxr::loadPreset, or `file`
		// with `from` (`path`, default: loadParams; `memory`: loadParamsMem
		// of the bytes, copied; `file`: loadParamsFile of a DiskFile). The
		// result code, or SO_NO_ERROR when neither is given.
		SoLoud::result loadSfxr(const Step &step, SoLoud::Sfxr &sfxr)
		{
			if (step.get("preset") && step.get("file"))
				throw ScenarioError(step.line, "`preset` and `file` cannot be combined");
			if (step.get("seed") && !step.get("preset"))
				throw ScenarioError(step.line, "`seed` needs `preset`");
			if (step.get("from") && !step.get("file"))
				throw ScenarioError(step.line, "`from` needs `file`");
			if (step.get("preset"))
				return sfxr.loadPreset((int)uintArg(step, "preset", true, 0), (int)uintArg(step, "seed", false, 0));
			if (!step.get("file"))
				return SoLoud::SO_NO_ERROR;
			std::string path = fixtureArg(step);
			std::string from = step.get("from") ? textArg(step, "from") : "path";
			if (from == "path")
				return sfxr.loadParams(path.c_str());
			if (from == "memory")
			{
				std::ifstream in(path.c_str(), std::ios::binary);
				std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
				return sfxr.loadParamsMem(bytes.empty() ? 0 : bytes.data(), (unsigned int)bytes.size(), true, false);
			}
			if (from == "file")
			{
				SoLoud::DiskFile file;
				SoLoud::result code = file.open(path.c_str());
				return code != SoLoud::SO_NO_ERROR ? code : sfxr.loadParamsFile(&file);
			}
			throw ScenarioError(step.line, "unknown from `" + from + "`");
		}

		// `file` with `from` (`path`, default: load; `memory`: loadMem of the
		// bytes, copied; `file`: loadFile of a DiskFile) for Monotone and
		// TedSid and Ay. The result code, or SO_NO_ERROR when no file is given.
		template <class T> SoLoud::result loadSongFile(const Step &step, T &song)
		{
			if (step.get("from") && !step.get("file"))
				throw ScenarioError(step.line, "`from` needs `file`");
			if (!step.get("file"))
				return SoLoud::SO_NO_ERROR;
			std::string path = fixtureArg(step);
			std::string from = step.get("from") ? textArg(step, "from") : "path";
			if (from == "path")
				return song.load(path.c_str());
			if (from == "memory")
			{
				std::ifstream in(path.c_str(), std::ios::binary);
				std::vector<unsigned char> bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
				return song.loadMem(bytes.empty() ? 0 : bytes.data(), (unsigned int)bytes.size(), true, false);
			}
			if (from == "file")
			{
				SoLoud::DiskFile file;
				SoLoud::result code = file.open(path.c_str());
				return code != SoLoud::SO_NO_ERROR ? code : song.loadFile(&file);
			}
			throw ScenarioError(step.line, "unknown from `" + from + "`");
		}

		// `hardware` and `waveform` (unsigned, passed as `int`s; each the
		// current value when not given) for Monotone::setParams, if either
		// is given. The result code, or SO_NO_ERROR.
		SoLoud::result monotoneParams(const Step &step, SoLoud::Monotone &song)
		{
			if (!step.get("hardware") && !step.get("waveform"))
				return SoLoud::SO_NO_ERROR;
			int hardware = (int)uintArg(step, "hardware", false, (unsigned int)song.mHardwareChannels);
			int waveform = (int)uintArg(step, "waveform", false, (unsigned int)song.mWaveform);
			return song.setParams(hardware, waveform);
		}

		static std::string hexText(const char *aText)
		{
			std::string out;
			char digits[4];
			for (const char *c = aText ? aText : ""; *c; c++)
			{
				std::snprintf(digits, sizeof(digits), "%02x", (unsigned int)(unsigned char)*c);
				out += digits;
			}
			return out;
		}

		// `tedsid` and `ay`: a new source, loaded as loadSongFile does.
		template <class T> void chipOp(const Step &step, T *tune)
		{
			static const char *const keys[] = { "name", "file", "from" };
			SoLoud::result code;
			std::string name;
			try
			{
				checkGeneratorKeys(step, keys, 3);
				engine(step);
				name = textArg(step, "name");
				code = loadSongFile(step, *tune);
			}
			catch (...)
			{
				delete tune;
				throw;
			}
			sourceSettings(step, *tune);
			storeGenerator(name, tune);
			std::ostringstream line;
			line << step.op << " " << name << " " << code;
			logLine(line);
		}

		static bool isGeneratorOp(const std::string &op)
		{
			return op == "noise" || op == "noise_set" || op == "vic" || op == "vic_set" || op == "vic_query" ||
				op == "sfxr" || op == "sfxr_load" || op == "sfxr_reset" || op == "sfxr_query" || op == "monotone" ||
				op == "monotone_set" || op == "monotone_query" || op == "tedsid" || op == "ay" ||
				op == "speech" || op == "speech_set" || op == "speech_query" || op == "vizsn";
		}

		void generatorOp(const Step &step)
		{
			static const char *const noiseKeys[] = { "name", "type", "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9" };
			if (step.op == "noise")
			{
				checkGeneratorKeys(step, noiseKeys, 12);
				engine(step);
				std::string name = textArg(step, "name");
				SoLoud::Noise *noise = new SoLoud::Noise;
				applyNoise(step, *noise);
				sourceSettings(step, *noise);
				storeGenerator(name, noise);
			}
			else if (step.op == "noise_set")
			{
				checkKeys(step, noiseKeys, 12);
				applyNoise(step, generatorArg<SoLoud::Noise>(step, "name", "noise"));
			}
			static const char *const sfxrKeys[] = { "name", "preset", "seed", "file", "from" };
			if (step.op == "sfxr")
			{
				checkGeneratorKeys(step, sfxrKeys, 5);
				engine(step);
				std::string name = textArg(step, "name");
				SoLoud::Sfxr *sfxr = new SoLoud::Sfxr;
				SoLoud::result code;
				try
				{
					code = loadSfxr(step, *sfxr);
				}
				catch (...)
				{
					delete sfxr;
					throw;
				}
				sourceSettings(step, *sfxr);
				storeGenerator(name, sfxr);
				std::ostringstream line;
				line << "sfxr " << name << " " << code;
				logLine(line);
			}
			else if (step.op == "sfxr_load")
			{
				checkKeys(step, sfxrKeys, 5);
				SoLoud::result code = loadSfxr(step, generatorArg<SoLoud::Sfxr>(step, "name", "sfxr"));
				std::ostringstream line;
				line << "sfxr_load " << textArg(step, "name") << " " << code;
				logLine(line);
			}
			else if (step.op == "sfxr_reset")
			{
				checkKeys(step, sfxrKeys, 1);
				generatorArg<SoLoud::Sfxr>(step, "name", "sfxr").resetParams();
			}
			else if (step.op == "sfxr_query")
			{
				checkKeys(step, sfxrKeys, 1);
				const SoLoud::SfxrParams &p = generatorArg<SoLoud::Sfxr>(step, "name", "sfxr").mParams;
				const float values[] = { p.p_base_freq, p.p_freq_limit, p.p_freq_ramp, p.p_freq_dramp, p.p_duty,
					p.p_duty_ramp, p.p_vib_strength, p.p_vib_speed, p.p_vib_delay, p.p_env_attack, p.p_env_sustain,
					p.p_env_decay, p.p_env_punch, p.p_lpf_resonance, p.p_lpf_freq, p.p_lpf_ramp, p.p_hpf_freq,
					p.p_hpf_ramp, p.p_pha_offset, p.p_pha_ramp, p.p_repeat_speed, p.p_arp_speed, p.p_arp_mod,
					p.master_vol, p.sound_vol };
				std::ostringstream line;
				line << "sfxr_query " << textArg(step, "name") << " wave=" << p.wave_type << " filter=" << (p.filter_on ? 1 : 0)
					<< " params=";
				for (size_t i = 0; i < sizeof(values) / sizeof(values[0]); i++)
					line << (i ? "," : "") << hex32(values[i]);
				logLine(line);
			}
			static const char *const vicKeys[] = { "name", "model", "bass", "alto", "soprano", "noise", "reg", "value" };
			if (step.op == "vic")
			{
				checkGeneratorKeys(step, vicKeys, 8);
				engine(step);
				std::string name = textArg(step, "name");
				SoLoud::Vic *vic = new SoLoud::Vic;
				try
				{
					applyVic(step, *vic);
				}
				catch (...)
				{
					delete vic;
					throw;
				}
				sourceSettings(step, *vic);
				storeGenerator(name, vic);
			}
			else if (step.op == "vic_set")
			{
				checkKeys(step, vicKeys, 8);
				applyVic(step, generatorArg<SoLoud::Vic>(step, "name", "vic"));
			}
			else if (step.op == "vic_query")
			{
				static const char *const keys[] = { "name", "reg" };
				checkKeys(step, keys, 2);
				SoLoud::Vic &vic = generatorArg<SoLoud::Vic>(step, "name", "vic");
				std::ostringstream line;
				line << "vic_query " << textArg(step, "name") << " model=" << vic.getModel() << " regs=";
				for (int i = 0; i < 4; i++)
					line << (i ? "," : "") << (unsigned int)vic.getRegister(i);
				if (step.get("reg"))
					line << " reg=" << (unsigned int)vic.getRegister((int)uintArg(step, "reg", true, 0));
				logLine(line);
			}
			static const char *const monotoneKeys[] = { "name", "file", "from", "hardware", "waveform" };
			if (step.op == "monotone")
			{
				checkGeneratorKeys(step, monotoneKeys, 5);
				engine(step);
				std::string name = textArg(step, "name");
				SoLoud::Monotone *song = new SoLoud::Monotone;
				SoLoud::result code, params;
				try
				{
					code = loadSongFile(step, *song);
					params = monotoneParams(step, *song);
				}
				catch (...)
				{
					delete song;
					throw;
				}
				sourceSettings(step, *song);
				storeGenerator(name, song);
				std::ostringstream line;
				line << "monotone " << name << " " << code << " " << params;
				logLine(line);
			}
			else if (step.op == "monotone_set")
			{
				static const char *const keys[] = { "name", "hardware", "waveform" };
				checkKeys(step, keys, 3);
				SoLoud::result code = monotoneParams(step, generatorArg<SoLoud::Monotone>(step, "name", "monotone"));
				std::ostringstream line;
				line << "monotone_set " << textArg(step, "name") << " " << code;
				logLine(line);
			}
			else if (step.op == "monotone_query")
			{
				checkKeys(step, monotoneKeys, 1);
				SoLoud::Monotone &song = generatorArg<SoLoud::Monotone>(step, "name", "monotone");
				std::ostringstream line;
				line << "monotone_query " << textArg(step, "name") << " hardware=" << song.mHardwareChannels
					<< " waveform=" << song.mWaveform << " patterns=" << (unsigned int)song.mSong.mTotalPatterns
					<< " tracks=" << (unsigned int)song.mSong.mTotalTracks << " title=" << hexText(song.mSong.mTitle)
					<< " comment=" << hexText(song.mSong.mComment) << " order=";
				char digits[4];
				for (int i = 0; i < 256; i++)
				{
					std::snprintf(digits, sizeof(digits), "%02x", (unsigned int)song.mSong.mOrder[i]);
					line << digits;
				}
				logLine(line);
			}
			if (step.op == "tedsid")
				chipOp(step, new SoLoud::TedSid);
			else if (step.op == "ay")
				chipOp(step, new SoLoud::Ay);
			static const char *const speechKeys[] = { "name", "text", "file" };
			static const char *const speechSetKeys[] = { "name", "frequency", "speed", "declination", "waveform" };
			if (step.op == "speech" || step.op == "vizsn")
			{
				checkGeneratorKeys(step, speechKeys, 3);
				engine(step);
				std::string name = textArg(step, "name");
				bool hasText = step.get("text") != 0;
				if (hasText && step.get("file"))
					throw ScenarioError(step.line, "`text` and `file` cannot be combined");
				std::string text;
				if (hasText)
				{
					// `_` for spaces.
					text = textArg(step, "text");
					for (size_t i = 0; i < text.size(); i++)
						if (text[i] == '_')
							text[i] = ' ';
				}
				else if (step.get("file"))
				{
					std::string path = fixtureArg(step);
					std::ifstream in(path.c_str(), std::ios::binary);
					if (!in)
						throw ScenarioError(step.line, "cannot read " + path);
					text.assign((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
					hasText = true;
				}
				SoLoud::result code = SoLoud::SO_NO_ERROR;
				if (step.op == "vizsn")
				{
					SoLoud::Vizsn *vizsn = new SoLoud::Vizsn;
					// setText reads up to the first 0.
					if (hasText)
						vizsn->setText(&text[0]);
					sourceSettings(step, *vizsn);
					storeGenerator(name, vizsn);
				}
				else
				{
					SoLoud::Speech *speech = new SoLoud::Speech;
					// setText reads up to the first 0.
					code = hasText ? speech->setText(text.c_str()) : SoLoud::SO_NO_ERROR;
					sourceSettings(step, *speech);
					storeGenerator(name, speech);
				}
				std::ostringstream line;
				line << step.op << " " << name << " " << code;
				logLine(line);
			}
			else if (step.op == "speech_set")
			{
				checkKeys(step, speechSetKeys, 5);
				SoLoud::Speech &speech = generatorArg<SoLoud::Speech>(step, "name", "speech");
				unsigned int frequency = uintArg(step, "frequency", false, (unsigned int)speech.mBaseFrequency);
				float speed = floatArg(step, "speed", speech.mBaseSpeed);
				float declination = floatArg(step, "declination", speech.mBaseDeclination);
				int waveform = speech.mBaseWaveform;
				if (step.get("waveform"))
				{
					unsigned int value = uintArg(step, "waveform", true, 0);
					if (value > 6)
						throw ScenarioError(step.line, "`waveform` must be 0 to 6");
					waveform = (int)value;
				}
				SoLoud::result code = speech.setParams(frequency, speed, declination, waveform);
				std::ostringstream line;
				line << "speech_set " << textArg(step, "name") << " " << code;
				logLine(line);
			}
			else if (step.op == "speech_query")
			{
				checkKeys(step, speechSetKeys, 1);
				SoLoud::Speech &speech = generatorArg<SoLoud::Speech>(step, "name", "speech");
				std::ostringstream line;
				line << "speech_query " << textArg(step, "name") << " frequency=" << (unsigned int)speech.mBaseFrequency
					<< " speed=" << hex32(speech.mBaseSpeed) << " declination=" << hex32(speech.mBaseDeclination)
					<< " waveform=" << speech.mBaseWaveform << " frames=" << speech.mFrames;
				logLine(line);
			}
		}

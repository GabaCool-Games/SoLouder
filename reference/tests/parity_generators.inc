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

		static bool isGeneratorOp(const std::string &op)
		{
			return op == "noise" || op == "noise_set" || op == "vic" || op == "vic_set" || op == "vic_query";
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
		}

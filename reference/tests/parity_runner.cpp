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
#include <sstream>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "soloud.h"

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

	struct Runner
	{
		SoLoud::Soloud soloud;
		bool initialized;
		std::vector<float> samples;
		std::vector<std::string> log;

		Runner() : initialized(false) {}

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
				static const char *const keys[] = { "channels", "rate", "buffer" };
				checkKeys(step, keys, 3);
				unsigned int channels = uintArg(step, "channels", false, 2);
				unsigned int rate = uintArg(step, "rate", false, SoLoud::Soloud::AUTO);
				unsigned int buffer = uintArg(step, "buffer", false, SoLoud::Soloud::AUTO);
				SoLoud::result code = soloud.init(SoLoud::Soloud::CLIP_ROUNDOFF, SoLoud::Soloud::NULLDRIVER, rate, buffer, channels);
				initialized = code == SoLoud::SO_NO_ERROR;
				std::ostringstream line;
				line << "init " << code;
				log.push_back(line.str());
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
				samples.resize(start + (size_t)frames * soloud.getBackendChannels());
				soloud.mix(samples.data() + start, frames);
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
			Runner *runner = new Runner();
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

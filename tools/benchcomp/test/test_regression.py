# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT
#
# Benchcomp regression testing suite. This suite uses Python's stdlib unittest
# module, but nevertheless actually runs the binary rather than running unit
# tests.

import pathlib
import re
import subprocess
import tempfile
import textwrap
import unittest
import uuid

import yaml


class Benchcomp:
    """Invocation of benchcomp binary with optional subcommand and flags"""

    def __init__(self, config):
        self.proc, self.stdout, self.stderr = None, None, None

        with tempfile.NamedTemporaryFile(
                mode="w", delete=False, suffix=".yaml") as tmp:
            yaml.dump(config, tmp, default_flow_style=False)
        self.config_file = tmp.name

        self.bc = str(pathlib.Path(__file__).parent.parent /
                      "bin" / "benchcomp")

        wd = tempfile.mkdtemp()
        self.working_directory = pathlib.Path(wd)

    def __call__(self, subcommand=None, default_flags=None, flags=None):
        subcommand = subcommand or []
        default_flags = default_flags or [
            "--out-prefix", "/tmp/benchcomp/test"]
        config_flags = ["--config", str(self.config_file)]

        flags = flags or []

        cmd = [self.bc, *config_flags, *subcommand, *default_flags, *flags]
        self.proc = subprocess.Popen(
            cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            cwd=self.working_directory)
        self.stdout, self.stderr = self.proc.communicate()


class RegressionTests(unittest.TestCase):
    def setUp(self):
        self.kani_dir = pathlib.Path(__file__).parent.parent.parent.parent

    def test_kani_perf_fail(self):
        cmd = (
            "rm -rf build target &&"
            "mkdir -p build/tests/perf/Unwind-Attribute/expected &&"
            "kani tests/kani/Unwind-Attribute/fixme_lib.rs "
            "--enable-unstable --cbmc-args --verbosity 9 > "
            "build/tests/perf/Unwind-Attribute/expected/expected.out"
        )
        self._run_kani_perf_test(cmd, False)

    def test_kani_perf_success(self):
        cmd = (
            "rm -rf build target &&"
            "mkdir -p build/tests/perf/Arbitrary/expected &&"
            "kani tests/kani/Arbitrary/arbitrary_impls.rs "
            "--enable-unstable --cbmc-args --verbosity 9 > "
            "build/tests/perf/Arbitrary/expected/expected.out"
        )
        self._run_kani_perf_test(cmd, True)

    def _run_kani_perf_test(self, command, expected_pass):
        """Ensure that the kani_perf parser can parse the output of a perf test"""

        # The two variants are identical; we're not actually checking the
        # returned metrics in this test, only checking that the parser works
        run_bc = Benchcomp({
            "variants": {
                "run_1": {
                    "config": {
                        "directory": str(self.kani_dir),
                        "command_line": command,
                    },
                },
                "run_2": {
                    "config": {
                        "directory": str(self.kani_dir),
                        "command_line": command,
                    },
                },
            },
            "run": {
                "suites": {
                    "suite_1": {
                        "parser": { "module": "kani_perf" },
                        "variants": ["run_1", "run_2"]
                    }
                }
            },
            "visualize": [{
                "type": "dump_yaml",
                "out_file": "-"
            }],
        })
        run_bc()
        self.assertEqual(run_bc.proc.returncode, 0, msg=run_bc.stderr)

        results = yaml.safe_load(run_bc.stdout)

        expected_types = {
            "solver_runtime": float,
            "symex_runtime": float,
            "verification_time": float,
            "success": bool,
            "number_program_steps": int,
            "number_vccs": int,
        }

        all_succeeded = True

        for _, bench in results["benchmarks"].items():
            for _, variant in bench["variants"].items():

                all_succeeded &= variant["metrics"]["success"]

                for metric, ttype in expected_types.items():
                    self.assertIn(metric, variant["metrics"], msg=run_bc.stdout)
                    self.assertTrue(
                        isinstance(variant["metrics"][metric], ttype),
                        msg=run_bc.stdout)

        self.assertEqual(expected_pass, all_succeeded, msg=run_bc.stdout)

    def test_error_on_regression_two_benchmarks_previously_failed(self):
        """Ensure that benchcomp terminates with exit of 0 when the "error_on_regression" visualization is configured and one of the benchmarks continues to fail (no regression)."""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "passed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 && "
                                "echo true > bench_1/success &&"
                                "echo false > bench_2/success"
                        },
                    },
                    "failed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 && "
                                "echo true > bench_1/success &&"
                                "echo false > bench_2/success"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["passed", "failed"]
                        }
                    }
                },
                "visualize": [{
                    "type": "error_on_regression",
                    "variant_pairs": [["passed", "failed"]],
                    "checks": [{
                        "metric": "success",
                        "test":
                            "lambda old, new: False if not old else not new"
                    }]
                }]
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)


    def test_error_on_regression_two_benchmarks_one_failed(self):
        """Ensure that benchcomp terminates with exit of 1 when the "error_on_regression" visualization is configured and one of the benchmarks' success metric has regressed"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "passed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 && "
                                "echo true > bench_1/success &&"
                                "echo true > bench_2/success"
                        },
                    },
                    "failed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 && "
                                "echo true > bench_1/success &&"
                                "echo false > bench_2/success"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["passed", "failed"]
                        }
                    }
                },
                "visualize": [{
                    "type": "error_on_regression",
                    "variant_pairs": [["passed", "failed"]],
                    "checks": [{
                        "metric": "success",
                        "test":
                            "lambda old, new: False if not old else not new"
                    }]
                }]
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 1, msg=run_bc.stderr)


    def test_error_on_regression_visualization_success_regressed(self):
        """Ensure that benchcomp terminates with exit of 1 when the "error_on_regression" visualization is configured and one of the benchmarks' success metric has regressed"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "passed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo true > bench_1/success"
                        },
                    },
                    "failed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo false > bench_1/success"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["passed", "failed"]
                        }
                    }
                },
                "visualize": [{
                    "type": "error_on_regression",
                    "variant_pairs": [["passed", "failed"]],
                    "checks": [{
                        "metric": "success",
                        "test":
                            "lambda old, new: False if not old else not new"
                    }]
                }]
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 1, msg=run_bc.stderr)


    def test_error_on_regression_visualization_success_no_regressed(self):
        """Ensure that benchcomp terminates with exit of 0 when the "error_on_regression" visualization is configured and none of the benchmarks' success metric has regressed"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "passed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo true > bench_1/success"
                        },
                    },
                    "failed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo true > bench_1/success"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["passed", "failed"]
                        }
                    }
                },
                "visualize": [{
                    "type": "error_on_regression",
                    "variant_pairs": [["passed", "failed"]],
                    "checks": [{
                        "metric": "success",
                        "test":
                            "lambda old, new: False if not old else not new"
                    }]
                }]
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)


    def test_error_on_regression_visualization_ratio_no_regressed(self):
        """Ensure that benchcomp terminates with exit of 0 when the "error_on_regression" visualization is configured and none of the metrics regressed"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "more": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo 10 > bench_1/n_bugs"
                        },
                    },
                    "less": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo 5 > bench_1/n_bugs"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["less", "more"]
                        }
                    }
                },
                "visualize": [{
                    "type": "error_on_regression",
                    "variant_pairs": [["more", "less"]],
                    "checks": [{
                        "metric": "n_bugs",
                        "test": "lambda old, new: new / old > 1.75",
                    }]
                }]
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)


    def test_error_on_regression_visualization_ratio_regressed(self):
        """Ensure that benchcomp terminates with exit of 1 when the "error_on_regression" visualization is configured and one of the metrics regressed"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "more": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo 10 > bench_1/n_bugs"
                        },
                    },
                    "less": {
                        "config": {
                            "directory": str(tmp),
                            "command_line": "mkdir bench_1 && echo 5 > bench_1/n_bugs"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["less", "more"]
                        }
                    }
                },
                "visualize": [{
                    "type": "error_on_regression",
                    "variant_pairs": [["less", "more"]],
                    "checks": [{
                        "metric": "n_bugs",
                        "test": "lambda old, new: new / old > 1.75",
                    }]
                }]
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 1, msg=run_bc.stderr)


    def test_markdown_results_table(self):
        """Run the markdown results table visualization"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "variant_1": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 bench_3"
                                "&& echo true > bench_1/success"
                                "&& echo true > bench_2/success"
                                "&& echo false > bench_3/success"
                                "&& echo 5 > bench_1/runtime"
                                "&& echo 10 > bench_2/runtime"
                        },
                    },
                    "variant_2": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 bench_3"
                                "&& echo true > bench_1/success"
                                "&& echo false > bench_2/success"
                                "&& echo true > bench_3/success"
                                "&& echo 10 > bench_1/runtime"
                                "&& echo 5 > bench_2/runtime"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["variant_1", "variant_2"]
                        }
                    }
                },
                "visualize": [{
                    "type": "dump_markdown_results_table",
                    "out_file": "-",
                    "scatterplot": "linear",
                    "extra_columns": {
                        "runtime": [{
                            "column_name": "ratio",
                            "text":
                                "lambda b: str(b['variant_2']/b['variant_1'])"
                                "if b['variant_2'] < 1.5 * b['variant_1'] "
                                "else '**' + str(b['variant_2']/b['variant_1']) + '**'"
                        }],
                        "success": [{
                            "column_name": "notes",
                            "text":
                                "lambda b: '' if b['variant_2'] == b['variant_1']"
                                "else 'newly passing' if b['variant_2'] "
                                "else 'regressed'"
                        }]
                    }
                }]
            })
            run_bc()

            self.assertEqual(run_bc.proc.returncode, 0, msg=run_bc.stderr)
            self.assertEqual(
                run_bc.stdout, textwrap.dedent("""
                    ## runtime

                    ```mermaid
                    %%{init: { "quadrantChart": { "titlePadding": 15, "xAxisLabelPadding": 20, "yAxisLabelPadding": 20, "quadrantLabelFontSize": 0, "pointRadius": 2, "pointLabelFontSize": 2 }, "themeVariables": { "quadrant1Fill": "#FFFFFF", "quadrant2Fill": "#FFFFFF", "quadrant3Fill": "#FFFFFF", "quadrant4Fill": "#FFFFFF", "quadrant1TextFill": "#FFFFFF", "quadrant2TextFill": "#FFFFFF", "quadrant3TextFill": "#FFFFFF", "quadrant4TextFill": "#FFFFFF", "quadrantInternalBorderStrokeFill": "#FFFFFF" } } }%%
                    quadrantChart
                        title runtime
                        x-axis "variant_1"
                        y-axis "variant_2"
                        quadrant-1 1
                        quadrant-2 2
                        quadrant-3 3
                        quadrant-4 4
                        "bench_1": [0.0, 0.909]
                        "bench_2": [0.909, 0.0]
                    ```
                    Scatterplot axis ranges are 5 (bottom/left) to 10 (top/right).

                    | Benchmark |  variant_1 | variant_2 | ratio |
                    | --- | --- | --- | --- |
                    | bench_1 | 5 | 10 | **2.0** |
                    | bench_2 | 10 | 5 | 0.5 |

                    ## success

                    | Benchmark |  variant_1 | variant_2 | notes |
                    | --- | --- | --- | --- |
                    | bench_1 | True | True |  |
                    | bench_2 | True | False | regressed |
                    | bench_3 | False | True | newly passing |
                    """))


    def test_only_dump_yaml(self):
        """Ensure that benchcomp terminates with return code 0 when `--only dump_yaml` is passed, even if the error_on_regression visualization would have resulted in a return code of 1"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "passed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 && "
                                "echo true > bench_1/success &&"
                                "echo true > bench_2/success"
                        },
                    },
                    "failed": {
                        "config": {
                            "directory": str(tmp),
                            "command_line":
                                "mkdir bench_1 bench_2 && "
                                "echo true > bench_1/success &&"
                                "echo false > bench_2/success"
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": { "module": "test_file_to_metric" },
                            "variants": ["passed", "failed"]
                        }
                    }
                },
                "visualize": [{
                    "type": "dump_yaml",
                    "out_file": "-",
                }, {
                    "type": "error_on_regression",
                    "variant_pairs": [["passed", "failed"]],
                    "checks": [{
                        "metric": "success",
                        "test":
                            "lambda old, new: True"
                    }]
                }]
            })
            run_bc(flags=["--only", "dump_yaml"])

            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)

            with open(run_bc.working_directory / "result.yaml") as handle:
                result = yaml.safe_load(handle)


    def test_ignore_dump_yaml(self):
        """Ensure that benchcomp does not print any YAML output even with the dump_yaml visualization when the `--except dump_yaml` flag is passed"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "variant_1": {
                        "config": {
                            "directory": tmp,
                            "command_line": "true",
                        }
                    },
                    "variant_2": {
                        "config": {
                            "directory": tmp,
                            "command_line": "true",
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {"module": "test"},
                            "variants": ["variant_1", "variant_2"]
                        }
                    }
                },
                "visualize": [{
                    "type": "dump_yaml",
                    "out_file": "-",
                }],
            })
            run_bc(flags=["--except", "dump_yaml"])

            self.assertEqual(
                run_bc.stdout, "", msg=run_bc.stdout)

            with open(run_bc.working_directory / "result.yaml") as handle:
                result = yaml.safe_load(handle)


    def test_return_0(self):
        """Ensure that benchcomp terminates with return code 0"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "variant_1": {
                        "config": {
                            "directory": tmp,
                            "command_line": "true",
                        }
                    },
                    "variant_2": {
                        "config": {
                            "directory": tmp,
                            "command_line": "true",
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {"module": "test"},
                            "variants": ["variant_1", "variant_2"]
                        }
                    }
                },
                "visualize": [],
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)

            with open(run_bc.working_directory / "result.yaml") as handle:
                result = yaml.safe_load(handle)


    def test_return_0_on_fail(self):
        """Ensure that benchcomp terminates with 0 even if a suite fails"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "variant_1": {
                        "config": {
                            "directory": tmp,
                            "command_line": "false",
                        }
                    },
                    "variant_2": {
                        "config": {
                            "directory": tmp,
                            "command_line": "true",
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {"module": "test"},
                            "variants": ["variant_1", "variant_2"]
                        }
                    }
                },
                "visualize": [],
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)

            with open(run_bc.working_directory / "result.yaml") as handle:
                result = yaml.safe_load(handle)


    def test_bad_filters(self):
        """Ensure that bad filters terminate benchcomp"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "variant-1": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                            "env": {},
                        }
                    },
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {
                                "command": textwrap.dedent("""\
                                    echo '{
                                        "benchmarks": { },
                                        "metrics": { }
                                    }'
                                    """)
                            },
                            "variants": ["variant-1"]
                        }
                    }
                },
                "filters": [{
                    "command_line": "false"
                }],
                "visualize": [],
            })
            run_bc()
            self.assertEqual(run_bc.proc.returncode, 1, msg=run_bc.stderr)


    def test_two_filters(self):
        """Ensure that the output can be filtered"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "variant-1": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                            "env": {},
                        }
                    },
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {
                                "command": textwrap.dedent("""\
                                    echo '{
                                        "benchmarks": {
                                            "bench-1": {
                                                "variants": {
                                                    "variant-1": {
                                                        "metrics": {
                                                            "runtime": 10,
                                                            "memory": 5
                                                        }
                                                    }
                                                }
                                            }
                                        },
                                        "metrics": {
                                            "runtime": {},
                                            "memory": {},
                                        }
                                    }'
                                    """)
                            },
                            "variants": ["variant-1"]
                        }
                    }
                },
                "filters": [{
                    "command_line": "sed -e 's/10/20/;s/5/10/'"
                }, {
                    "command_line": """grep '"runtime": 20'"""
                }],
                "visualize": [],
            })
            run_bc()
            self.assertEqual(run_bc.proc.returncode, 0, msg=run_bc.stderr)


    def test_env_expansion(self):
        """Ensure that config parser expands '${}' in env key"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "env_set": {
                        "config": {
                            "command_line": 'echo "$__BENCHCOMP_ENV_VAR" > out',
                            "directory": tmp,
                            "env": {"__BENCHCOMP_ENV_VAR": "foo:${PATH}"}
                        }
                    },
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {
                                # The word 'bin' typically appears in $PATH, so
                                # check that what was echoed contains 'bin'.
                                "command": textwrap.dedent("""\
                                    grep bin out && grep '^foo:' out && echo '{
                                        "benchmarks": {},
                                        "metrics": {}
                                    }'
                                    """)
                            },
                            "variants": ["env_set"]
                        }
                    }
                },
                "visualize": [],
            })
            run_bc()
            self.assertEqual(run_bc.proc.returncode, 0, msg=run_bc.stderr)


    def test_env(self):
        """Ensure that benchcomp reads the 'env' key of variant config"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "env_set": {
                        "config": {
                            "command_line": "echo $QJTX > out",
                            "directory": tmp,
                            "env": {"QJTX": "foo"}
                        }
                    },
                    "env_unset": {
                        "config": {
                            "command_line": "echo $QJTX > out",
                            "directory": tmp,
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {"module": "test"},
                            "variants": ["env_unset", "env_set"]
                        }
                    }
                },
                "visualize": [],
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)

            with open(run_bc.working_directory / "result.yaml") as handle:
                result = yaml.safe_load(handle)

            self.assertEqual(
                result["benchmarks"]["suite_1"]["variants"][
                    "env_set"]["metrics"]["foos"], 1,
                msg=yaml.dump(result, default_flow_style=False))

            self.assertEqual(
                result["benchmarks"]["suite_1"]["variants"][
                    "env_unset"]["metrics"]["foos"], 0,
                msg=yaml.dump(result, default_flow_style=False))


    def test_command_parser(self):
        """Ensure that CommandParser can execute and read the output of a parser"""

        with tempfile.TemporaryDirectory() as tmp:
            run_bc = Benchcomp({
                "variants": {
                    "v1": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    },
                    "v2": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {
                                "command": """
                                    echo '{
                                        "benchmarks": {},
                                        "metrics": {}
                                    }'
                                """
                            },
                            "variants": ["v2", "v1"]
                        }
                    }
                },
                "visualize": [],
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)

            with open(run_bc.working_directory / "result.yaml") as handle:
                result = yaml.safe_load(handle)

            for item in ["benchmarks", "metrics"]:
                self.assertIn(item, result)


    def test_run_command_visualization(self):
        """Ensure that the run_command visualization can execute a command"""

        with tempfile.TemporaryDirectory() as tmp:
            out_file = pathlib.Path(tmp) / str(uuid.uuid4())
            run_bc = Benchcomp({
                "variants": {
                    "v1": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    },
                    "v2": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {
                                "command": """
                                    echo '{
                                        "benchmarks": {},
                                        "metrics": {}
                                    }'
                                """
                            },
                            "variants": ["v2", "v1"]
                        }
                    }
                },
                "visualize": [{
                    "type": "run_command",
                    "command": f"cat - > {out_file}"
                }],
            })
            run_bc()
            self.assertEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)

            with open(out_file) as handle:
                result = yaml.safe_load(handle)

            for item in ["benchmarks", "metrics"]:
                self.assertIn(item, result)


    def test_run_failing_command_visualization(self):
        """Ensure that benchcomp terminates with a non-zero return code when run_command visualization fails"""

        with tempfile.TemporaryDirectory() as tmp:
            out_file = pathlib.Path(tmp) / str(uuid.uuid4())
            run_bc = Benchcomp({
                "variants": {
                    "v1": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    },
                    "v2": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {
                                "command": """
                                    echo '{
                                        "benchmarks": {},
                                        "metrics": {}
                                    }'
                                """
                            },
                            "variants": ["v2", "v1"]
                        }
                    }
                },
                "visualize": [{
                    "type": "run_command",
                    "command": f"cat - > {out_file}; false"
                }],
            })
            run_bc()
            self.assertNotEqual(
                run_bc.proc.returncode, 0, msg=run_bc.stderr)


    def test_unknown_metric_in_benchmark(self):
        """Ensure that benchcomp continues with warning if a benchmark result contained an unknown metric"""

        with tempfile.TemporaryDirectory() as tmp:
            out_file = pathlib.Path(tmp) / str(uuid.uuid4())
            run_bc = Benchcomp({
                "variants": {
                    "v1": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    },
                    "v2": {
                        "config": {
                            "command_line": "true",
                            "directory": tmp,
                        }
                    }
                },
                "run": {
                    "suites": {
                        "suite_1": {
                            "parser": {
                                "command": """
                                    echo '{
                                      metrics: {
                                        foo: {},
                                        bar: {},
                                      },
                                      benchmarks: {
                                        bench_1: {
                                          metrics: {
                                            baz: 11
                                          }
                                        }
                                      }
                                    }'
                                """
                            },
                            "variants": ["v2", "v1"]
                        }
                    }
                },
                "visualize": [{
                    "type": "dump_markdown_results_table",
                    "out_file": "-",
                    "extra_columns": {},
                }],
            })

            output_pat = re.compile(
                "Benchmark 'bench_1' contained a metric 'baz' in the 'v1' "
                "variant result that was not declared in the 'metrics' dict.")

            run_bc()
            self.assertRegex(run_bc.stderr, output_pat)

            self.assertEqual(run_bc.proc.returncode, 0, msg=run_bc.stderr)

            with open(run_bc.working_directory / "result.yaml") as handle:
                result = yaml.safe_load(handle)

            for item in ["benchmarks", "metrics"]:
                self.assertIn(item, result)

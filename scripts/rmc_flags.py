#!/usr/bin/env python3
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0 OR MIT

import argparse

# Add flags related to debugging output.
def add_loudness_flags(make_group, add_flag):
    group = make_group(
        "Loudness flags", "Determine how much textual output to produce.")
    add_flag(group, "--debug", action="store_true",
             help="Produce full debug information")
    add_flag(group, "--verbose", "-v", action="store_true",
             help="Output processing stages and commands, along with minor debug information")
    add_flag(group, "--quiet", "-q", action="store_true",
             help="Produces no output, just an exit code and requested artifacts. Overrides --verbose")

# Add flags which specify configurations for the proof.
def add_proof_harness_flags(make_group, add_flag):
    group = make_group("Proof harness flags",
                       "Provide information about the proof harness for RMC.")
    add_flag(group, "--c-lib", nargs="*", default=[], action="extend",
             help="External C files referenced by Rust code")
    add_flag(group, "--function", default="main",
             help="Entry point for verification")

# Add flags that produce extra artifacts.
def add_artifact_flags(make_group, add_flag):
    group = make_group(
        "Artifact flags", "Produce artifacts in addition to a basic RMC report.")
    add_flag(group, "--target-dir", default=".",
             help="Directory for all generated artifacts")
    add_flag(group, "--keep-temps", action="store_true",
             help="Keep temporary files generated throughout RMC process")
    add_flag(group, "--gen-c", action="store_true",
             help="Generate C file equivalent to inputted program")
    add_flag(group, "--gen-symbols", action="store_true",
             help="Generate a symbol table")

# Add flags to turn off default checks.
def add_check_flags(make_group, add_flag):
    group = make_group("Check flags", "Disable some or all default checks.")
    add_flag(group, "--no-default-checks", action="store_true",
             help="Disable all default checks")
    add_flag(group, "--no-memory-safety-checks", action="store_true",
             help="Disable default memory safety checks")
    add_flag(group, "--no-overflow-checks", action="store_true",
             help="Disable default overflow checks")
    add_flag(group, "--no-unwinding-checks", action="store_true",
             help="Disable default unwinding checks")

# Add flags needed only for visualizer.
def add_visualizer_flags(make_group, add_flag):
    group = make_group(
        "Visualizer flags", "Generate an HTML-based UI for the generated RMC report.\nSee https://github.com/awslabs/aws-viewer-for-cbmc.")
    add_flag(group, "--srcdir", default=".",
             help="The source directory. The root of the source tree.")
    add_flag(group, "--wkdir", default=".",
             help="""
                  The working directory. Used to determine source locations in output.
                  This is generally the location from which rmc is currently being invoked.
                  """)
    add_flag(group, "--visualize", action="store_true",
             help="Generate visualizer report; open report/html/index.html")

# Add flags for ad-hoc features.
def add_other_flags(make_group, add_flag):
    group = make_group("Other flags")
    add_flag(group, "--allow-cbmc-verification-failure", action="store_true",
             help="Do not produce error return code on CBMC verification failure")
    add_flag(group, "--mangler", default="v0",
             help="Change what mangler is used by the Rust compiler")
    add_flag(group, "--dry-run", action="store_true",
             help="Print commands instead of running them")

# Add flags we don't expect end-users to use.
def add_developer_flags(make_group, add_flag):
    group = make_group(
        "Developer flags", "These are generally meant for use by RMC developers, and are not stable.")
    add_flag(group, "--cbmc-args", nargs=argparse.REMAINDER, default=[],
             help="Pass through directly to CBMC; must be the last flag")

# Adds the flags common to both rmc and cargo-rmc.
# Allows you to specify flags/groups of flags to not add.
# This does not return the parser, but mutates the one provided.
def add_flags(parser, exclude_flags=[], exclude_groups=[]):
    # Keep track of what excluded flags and groups we've seen
    # so we can warn for possibly incorrect names passed in.
    excluded_flags = set()
    excluded_groups = set()

    # Add a group to the parser with title/description, and get a handler for it.
    def make_group(title=None, description=None):
        if title in exclude_groups:
            excluded_groups.add(group.title)
            return None

        return parser.add_argument_group(title, description)

    # Add the flag to the group, 
    def add_flag(group, flag, *args, **kwargs):
        if group == None:
            return

        if flag in exclude_flags:
            excluded_flags.add(flag)
            return

        group.add_argument(flag, *args, **kwargs)

    add_groups = [
        add_loudness_flags,
        add_proof_harness_flags,
        add_artifact_flags,
        add_check_flags,
        add_visualizer_flags,
        add_other_flags,
        add_developer_flags
    ]

    for add_group in add_groups:
        add_group(make_group, add_flag)

    # Error if any excluded flags/groups don't exist.
    extra_groups = set(exclude_groups) - excluded_groups
    if extra_groups:
        print(f"ERROR: Attempt to exclude groups which don't exist: {extra_groups}")

    extra_flags = set(exclude_flags) - excluded_flags
    if extra_flags:
        print(f"ERROR: Attempt to exclude flags which don't exist: {extra_flags}")

    if extra_groups or extra_flags:
        return False

    return True

#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

cargo kani autoharness -Z autoharness -Z function-contracts -Z loop-contracts
# We expect verification to fail, so the above command will produce an exit status of 1
# However, we don't want the test to fail because of that exit status; we only want it to fail if the expected file doesn't match
# So, exit with a status code of 0 explicitly.
exit 0;

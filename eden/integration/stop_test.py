#!/usr/bin/env python3
#
# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

import contextlib
import os
import pathlib
import signal
import sys
import time
import typing

import pexpect
from eden.cli.daemon import did_process_exit
from eden.cli.util import poll_until

from .lib.find_executables import FindExe
from .lib.pexpect import PexpectAssertionMixin
from .lib.service_test_case import ServiceTestCaseBase, service_test
from .lib.temporary_directory import TemporaryDirectoryMixin


SHUTDOWN_EXIT_CODE_NORMAL = 0
SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN = 0
SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR = 2
SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL = 3


@service_test
class StopTest(ServiceTestCaseBase, PexpectAssertionMixin, TemporaryDirectoryMixin):
    def setUp(self):
        super().setUp()
        self.tmp_dir = self.make_temporary_directory()

    def test_stop_stops_running_daemon(self):
        with self.spawn_fake_edenfs(pathlib.Path(self.tmp_dir)) as daemon_pid:
            stop_process = self.spawn_stop(["--timeout", "5"])
            stop_process.expect_exact("edenfs exited cleanly.")
            self.assert_process_exit_code(stop_process, SHUTDOWN_EXIT_CODE_NORMAL)
            self.assertTrue(
                did_process_exit(daemon_pid), f"Process {daemon_pid} should have died"
            )

    def test_stop_sigkill(self):
        with self.spawn_fake_edenfs(pathlib.Path(self.tmp_dir), ["--ignoreStop"]):
            # Ask the CLI to stop edenfs, with a 1 second timeout.
            # It should have to kill the process with SIGKILL
            stop_process = self.spawn_stop(["--timeout", "1"])
            stop_process.expect_exact("Terminated edenfs with SIGKILL")
            self.assert_process_exit_code(
                stop_process, SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
            )

    def test_async_stop_stops_daemon_eventually(self):
        with self.spawn_fake_edenfs(pathlib.Path(self.tmp_dir)) as daemon_pid:
            stop_process = self.spawn_stop(["--timeout", "0"])
            stop_process.expect_exact("Sent async shutdown request to edenfs.")
            self.assert_process_exit_code(
                stop_process, SHUTDOWN_EXIT_CODE_REQUESTED_SHUTDOWN
            )

            def daemon_exited() -> typing.Optional[bool]:
                if did_process_exit(daemon_pid):
                    return True
                else:
                    return None

            poll_until(daemon_exited, timeout=10)

    def test_stop_not_running(self):
        stop_process = self.spawn_stop(["--timeout", "1"])
        stop_process.expect_exact("edenfs is not running")
        self.assert_process_exit_code(
            stop_process, SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR
        )

    def test_stopping_killed_daemon_reports_not_running(self):
        daemon = self.spawn_fake_edenfs(pathlib.Path(self.tmp_dir))
        os.kill(daemon.process_id, signal.SIGKILL)

        stop_process = self.spawn_stop(["--timeout", "1"])
        stop_process.expect_exact("edenfs is not running")
        self.assert_process_exit_code(
            stop_process, SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR
        )

    def test_killing_hung_daemon_during_stop_makes_stop_finish(self):
        with self.spawn_fake_edenfs(pathlib.Path(self.tmp_dir)) as daemon_pid:
            os.kill(daemon_pid, signal.SIGSTOP)
            try:
                stop_process = self.spawn_stop(["--timeout", "5"])

                time.sleep(2)
                self.assertTrue(
                    stop_process.isalive(),
                    "'eden stop' should wait while daemon is hung",
                )

                os.kill(daemon_pid, signal.SIGKILL)

                stop_process.expect_exact("error: edenfs is not running")
                self.assert_process_exit_code(
                    stop_process, SHUTDOWN_EXIT_CODE_NOT_RUNNING_ERROR
                )
            finally:
                with contextlib.suppress(ProcessLookupError):
                    os.kill(daemon_pid, signal.SIGCONT)

    def test_stopping_daemon_stopped_by_sigstop_kills_daemon(self):
        with self.spawn_fake_edenfs(pathlib.Path(self.tmp_dir)) as daemon_pid:
            os.kill(daemon_pid, signal.SIGSTOP)
            try:
                stop_process = self.spawn_stop(["--timeout", "1"])
                stop_process.expect_exact("warning: edenfs is not responding")
                self.assert_process_exit_code(
                    stop_process, SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
                )
            finally:
                with contextlib.suppress(ProcessLookupError):
                    os.kill(daemon_pid, signal.SIGCONT)

    def test_hanging_thrift_call_kills_daemon_with_sigkill(self):
        with self.spawn_fake_edenfs(
            pathlib.Path(self.tmp_dir), ["--sleepBeforeStop=5"]
        ):
            stop_process = self.spawn_stop(["--timeout", "1"])
            self.assert_process_exit_code(
                stop_process, SHUTDOWN_EXIT_CODE_TERMINATED_VIA_SIGKILL
            )

    def test_stop_succeeds_if_thrift_call_abruptly_kills_daemon(self):
        with self.spawn_fake_edenfs(
            pathlib.Path(self.tmp_dir), ["--exitWithoutCleanupOnStop"]
        ):
            stop_process = self.spawn_stop(["--timeout", "10"])
            self.assert_process_exit_code(stop_process, SHUTDOWN_EXIT_CODE_NORMAL)

    def spawn_stop(self, extra_args: typing.List[str]) -> "pexpect.spawn[str]":
        return pexpect.spawn(
            FindExe.EDEN_CLI,
            ["--config-dir", self.tmp_dir, "stop"] + extra_args,
            encoding="utf-8",
            logfile=sys.stderr,
        )

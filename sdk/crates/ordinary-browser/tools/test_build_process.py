"""Local process-lifecycle regression. Uses only owned disposable child processes."""
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
import build_browser


class ProcessGroupTests(unittest.TestCase):
    def test_timeout_kills_compiler_child_after_parent_exits(self):
        with tempfile.TemporaryDirectory(prefix='dyt-build-lifecycle-') as directory:
            root=Path(directory); marker=root/'child.pid'
            child = 'import signal,time;signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)'
            parent = ('import subprocess,sys,time;from pathlib import Path;'
                      'p=subprocess.Popen([sys.executable,"-c",sys.argv[2]]);'
                      'Path(sys.argv[1]).write_text(str(p.pid));time.sleep(60)')
            original_stop=build_browser.stop_process_group
            with patch.object(build_browser, 'stop_process_group', side_effect=lambda p:original_stop(p,0.5)):
                with self.assertRaisesRegex(RuntimeError,'time bound'):
                    build_browser.run([sys.executable,'-c',parent,str(marker),child],dict(os.environ),root/'build.log',timeout=0.5)
            self.assertTrue(marker.exists())
            child_pid=int(marker.read_text())
            deadline=time.monotonic()+5
            while time.monotonic()<deadline:
                result=subprocess.run(['/bin/ps','-o','stat=','-p',str(child_pid)],capture_output=True,text=True,timeout=2)
                if result.returncode or not result.stdout.strip() or result.stdout.strip().startswith('Z'):
                    break
                time.sleep(0.05)
            else:
                os.kill(child_pid,signal.SIGKILL)
                self.fail('Compiler child remained active after timeout cleanup')

if __name__=='__main__': unittest.main(verbosity=2)

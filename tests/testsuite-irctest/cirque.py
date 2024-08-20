import os
from typing import Any, Optional, Type

from irctest.basecontrollers import BaseServerController, DirectoryBasedController


class CirqueController(BaseServerController, DirectoryBasedController):
    software_name = "cirque"

    def run(
        self,
        hostname: str,
        port: int,
        *,
        password: Optional[str],
        ssl: bool,
        run_services: bool,
        faketime: Optional[str],
        config: Optional[Any] = None,
    ) -> None:
        args = ["-p", str(port), "--server-name", "My.Little.Server"]
        bin = os.path.join(os.getcwd(), "target/debug/irctest-compat")
        self.debug_mode = True
        self.proc = self.execute([bin] + args)
        self.port = port


def get_irctest_controller_class() -> Type[CirqueController]:
    return CirqueController

#!/usr/bin/env bash

set -eu

if [ ! -d irctest ]; then
    git clone https://github.com/progval/irctest
    cd irctest && git reset --hard d202e440bb71139e7c10e25bd04660866371c686 && cd ..
fi

uv pip install -r irctest/requirements.txt pytest-xdist

cargo build --bin irctest-compat

filters="not Ergo and not IRCv3 and not deprecated"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/pingpong.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/join.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/part.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/names.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/topic.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/channel.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/chmodes/operator.py -k "$filters"

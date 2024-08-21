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
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/away.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/connection_registration.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/whois.py -k "$filters and not testWhoisNumerics[oper]"  # we don't support OPER
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/who.py -k "$filters and not Oper and not Invisible and not [mask]"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/lusers.py -k "$filters"
PYTHONPATH=tests/testsuite-irctest/ pytest -v -n 3 --controller cirque irctest/irctest/server_tests/regressions.py -k "$filters"

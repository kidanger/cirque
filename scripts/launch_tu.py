import subprocess
import os

if __name__ == "__main__":
    subprocess.run(["cargo", "build", "--bin", "chirc-compat"], check=True)

    if not os.path.exists("chirc"):
        subprocess.run(
            ["git", "clone", "https://github.com/uchicago-cs/chirc"], check=True
        )
        subprocess.run(
            ["git", "reset", "--hard", "a392e1789c362e58c75b0bc533fc0aeac6f56304"],
            cwd="chirc",
            check=True,
        )

    file_name = "chirc-compat"
    if os.name == "nt":
        file_name = file_name + ".exe"

    exe_path = os.path.join("target", "debug", file_name)
    for tu_type in [
        "BASIC_CONNECTION",
        "CHANNEL_JOIN",
        "CHANNEL_PRIVMSG_NOTICE",
        "CHANNEL_PART",
        "PING_PONG",
        "QUIT_CHANNEL",
        "NICK_CHANNEL",
        "CONNECTION_REGISTRATION",
        "PRIVMSG_NOTICE",
        "MOTD",
        # disabled:
        # "ERR_UNKNOWN",
        #   because WHOWAS is not handled or something like that
    ]:
        subprocess.run(
            [
                "pytest",
                "chirc",
                "--disable-pytest-warnings",
                "-vv",
                "--chirc-exe",
                exe_path,
                "--chirc-category",
                tu_type,
            ],
            check=True,
        )

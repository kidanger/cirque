import subprocess
import os

if __name__ == "__main__":

    subprocess.call(["cargo", "build", "--bin", "chirc-compat"])
    if os.path.exists("chirc") == False:
        subprocess.call(["git", "clone" ,"https://github.com/uchicago-cs/chirc"])
        subprocess.call(["git", "reset", "--hard", "a392e1789c362e58c75b0bc533fc0aeac6f56304"], cwd="chirc")
    
    file_name = "chirc-compat"
    if os.name == 'nt':
        file_name = file_name + ".exe"

    exe_path = os.path.join("target", "debug", file_name)
    for tu_type in ["BASIC_CONNECTION", "ERR_UNKNOWN", "CHANNEL_JOIN", "CHANNEL_PRIVMSG_NOTICE", "CHANNEL_PART", "PING_PONG"]:
        subprocess.call(["pytest" ,"chirc" ,"--disable-pytest-warnings" ,"-vv" ,"--chirc-exe", exe_path, "--chirc-category", tu_type])





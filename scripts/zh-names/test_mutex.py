"""验证 run_hant_job 的单实例互斥：持锁时第二个实例应立即退出。"""

import ctypes
import subprocess

PY = r"C:\Users\joeyl\.workbuddy\binaries\python\envs\default\Scripts\python.exe"
SCRIPT = r"C:\Users\joeyl\Desktop\Project：Game-Manage-Center\Game-Manage-Center\scripts\zh-names\run_hant_job.py"

kernel32 = ctypes.windll.kernel32
kernel32.CreateMutexW.restype = ctypes.c_void_p
kernel32.CreateMutexW.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_char_p]

# 用与 run_hant_job 相同的名字先持锁
handle = kernel32.CreateMutexW(None, False, "Global\\gm_hant_job".encode("utf-8"))
print("模拟已运行实例持锁:", bool(handle))

result = subprocess.run(
    [PY, "-u", SCRIPT],
    capture_output=True,
    text=True,
    encoding="utf-8",
    errors="replace",
    timeout=60,
)
output = (result.stdout or "") + (result.stderr or "")
print("第二实例输出尾部:")
for line in output.strip().splitlines()[-3:]:
    print("  " + line)
verdict = "互斥生效" if "已有繁中作业在运行" in output else "互斥未生效"
print(verdict)

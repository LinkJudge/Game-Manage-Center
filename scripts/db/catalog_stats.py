#!/usr/bin/env python3
"""只读统计当前目录快照；路径覆盖按数组非空计算。"""
import argparse
import json
import sqlite3
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--db",type=Path,default=ROOT/"src-tauri/resources/database.db")
    args=parser.parse_args()
    conn=sqlite3.connect(args.db.resolve().as_uri()+"?mode=ro",uri=True)
    try:
        rows=conn.execute("SELECT save_paths,name_zh,name_zh_hant FROM games").fetchall()
        paths=[json.loads(r[0] or "{}") for r in rows]
        result={"games":len(rows),"with_paths":sum(any(p.get(k) for k in ("win","reg","mac","linux")) for p in paths),
                "with_windows_or_registry":sum(bool(p.get("win") or p.get("reg")) for p in paths),
                "paths_by_os":{k:sum(bool(p.get(k)) for p in paths) for k in ("win","reg","mac","linux")},
                "zh":sum(bool((r[1] or "").strip()) for r in rows),"hant":sum(bool((r[2] or "").strip()) for r in rows),
                "steam_names":conn.execute("SELECT COUNT(*) FROM steam_names").fetchone()[0],
                "xiaoheihe":conn.execute("SELECT COUNT(*) FROM name_sources WHERE source='xiaoheihe'").fetchone()[0]}
        print(json.dumps(result,ensure_ascii=False,indent=2))
    finally:
        conn.close()
if __name__=="__main__":main()

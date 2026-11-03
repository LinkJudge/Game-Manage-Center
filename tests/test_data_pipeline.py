"""数据库导出和采集保护的离线回归，不访问任何第三方接口。"""
import hashlib
import sqlite3
import sys
import tempfile
import unittest
from datetime import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path[:0] = [str(ROOT/"scripts/db"),str(ROOT/"scripts/zh-names")]
from make_release_db import export_database, connection
from collection_state import BudgetExceeded, daily_usage, reserve_request, single_collector, validate_target

class PipelineTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.source = self.root/"full.db"
        self.out = self.root/"release.db"
        self.backups = self.root/"backups"
        with connection(self.source) as conn:
            conn.executescript("""
            CREATE TABLE games(id INTEGER PRIMARY KEY,pcgw_id INTEGER UNIQUE,name_en TEXT,name_zh TEXT,name_zh_hant TEXT,install_dir TEXT,steam_appid INTEGER,gog_appid INTEGER,save_paths TEXT,stores TEXT);
            INSERT INTO games VALUES(1,100,'Game','简体','繁體','Game',123,456,'{"win":["x"]}','["Steam"]');
            CREATE TABLE name_sources(pcgw_id INTEGER,source TEXT,checked_at TEXT);
            CREATE TABLE steam_raw(steam_appid TEXT,payload BLOB);
            INSERT INTO steam_raw VALUES('123',X'1234');
            CREATE TABLE xhh_raw(pcgw_id INTEGER,payload BLOB);
            INSERT INTO xhh_raw VALUES(100,X'1234');
            """)
    def digest(self,p):
        return hashlib.sha256(p.read_bytes()).hexdigest()
    def test_export_keeps_source_and_supports_old_and_new_queries(self):
        before=self.digest(self.source)
        result=export_database(self.source,self.out,self.backups)
        self.assertEqual(result["games"],1)
        self.assertEqual(before,self.digest(self.source))
        with connection(self.out) as conn:
            new=conn.execute("SELECT name_en,pcgw_id,install_dir,steam_appid,gog_appid,stores,name_zh,save_paths FROM games").fetchall()
            old=conn.execute("SELECT title,wiki_page_id,install_folder,steam_id,gog_id,platform,zh_CN,save_location FROM games").fetchall()
            self.assertEqual(new,old)
            self.assertEqual(conn.execute("SELECT count(*) FROM sqlite_master WHERE name IN ('steam_raw','xhh_raw')").fetchone()[0],0)
        export_database(self.source,self.out,self.backups)
        self.assertEqual(len(list(self.backups.glob("*.bak"))),1)
    def test_active_writer_aborts_without_replacing_output(self):
        export_database(self.source,self.out,self.backups);before=self.digest(self.out)
        with connection(self.source) as lock:
            lock.execute("BEGIN IMMEDIATE")
            with self.assertRaises(sqlite3.OperationalError):
                export_database(self.source,self.out,self.backups)
            self.assertEqual(before,self.digest(self.out))
    def test_same_path_and_empty_input_are_rejected(self):
        with self.assertRaises(ValueError):export_database(self.source,self.source,self.backups)
        with connection(self.source) as conn:conn.execute("DELETE FROM games")
        with self.assertRaises(ValueError):export_database(self.source,self.out,self.backups)
        self.assertFalse(self.out.exists())
    def test_daily_budget_counts_failed_attempts_and_survives_restart(self):
        with connection(self.source) as conn:
            conn.execute("INSERT INTO name_sources VALUES(100,'xiaoheihe',?)",(datetime.now().isoformat(),))
        reserve_request(self.source,limit=3);reserve_request(self.source,limit=3)
        with self.assertRaises(BudgetExceeded):reserve_request(self.source,limit=3)
        with connection(self.source) as conn:self.assertEqual(daily_usage(conn),3)
    def test_process_lock_excludes_second_collector(self):
        lock=self.root/"collector.lock"
        with single_collector(lock):
            with self.assertRaises(RuntimeError):
                with single_collector(lock):pass
        with single_collector(lock):pass
    def test_collection_refuses_bundled_database(self):
        with self.assertRaises(ValueError):validate_target(ROOT/"src-tauri/resources/database.db")
        self.assertEqual(validate_target(self.source),self.source)

if __name__=="__main__":unittest.main()

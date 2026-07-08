use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

use crate::shared::crypto::Crypto;
use crate::shared::models::{calculate_strength, PasswordDto, PasswordInput};

/// 数据库管理器
pub struct Database {
    conn: Mutex<Connection>,
    crypto: Crypto,
}

#[allow(dead_code)]
impl Database {
    /// 打开或创建数据库
    pub fn open(db_path: &Path, crypto: Crypto) -> Result<Self, String> {
        let conn = Connection::open(db_path)
            .map_err(|e| format!("打开数据库失败: {}", e))?;

        // 启用 WAL 模式提升并发性能
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("设置数据库参数失败: {}", e))?;

        // 创建表
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS passwords (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                icon TEXT DEFAULT '🔑',
                url TEXT DEFAULT '',
                username TEXT DEFAULT '',
                password_encrypted TEXT NOT NULL,
                tags TEXT DEFAULT '[]',
                notes TEXT DEFAULT '',
                favorite INTEGER DEFAULT 0,
                strength INTEGER DEFAULT 0,
                created TEXT DEFAULT '',
                last_used TEXT DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS app_config (
                key TEXT PRIMARY KEY,
                value TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_passwords_name ON passwords(name);
            CREATE INDEX IF NOT EXISTS idx_passwords_favorite ON passwords(favorite);",
        )
        .map_err(|e| format!("创建表失败: {}", e))?;

        Ok(Database {
            conn: Mutex::new(conn),
            crypto,
        })
    }

    /// 获取数据库文件路径（用于同步）
    pub fn db_path(&self) -> Option<std::path::PathBuf> {
        None // 由 AppState 管理
    }

    /// 获取所有密码（已解密）
    pub fn get_all(&self) -> Result<Vec<PasswordDto>, String> {
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT id, name, icon, url, username, password_encrypted, tags, notes, favorite, strength, created, last_used FROM passwords ORDER BY name COLLATE NOCASE")
            .map_err(|e| format!("查询失败: {}", e))?;

        let rows = stmt
            .query_map([], |row| self.row_to_dto(row))
            .map_err(|e| format!("读取数据失败: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("行数据错误: {}", e))?);
        }
        Ok(result)
    }

    /// 搜索密码（名字不完全匹配，支持名称/用户名/网址/标签/备注）
    pub fn search(&self, query: &str) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        let q = query.to_lowercase();
        let filtered: Vec<PasswordDto> = all
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&q)
                    || p.username.to_lowercase().contains(&q)
                    || p.url.to_lowercase().contains(&q)
                    || p.notes.to_lowercase().contains(&q)
                    || p.tags.iter().any(|t| t.to_lowercase().contains(&q))
            })
            .collect();
        Ok(filtered)
    }

    /// 按标签筛选
    pub fn get_by_tag(&self, tag: &str) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        Ok(all.into_iter().filter(|p| p.tags.iter().any(|t| t == tag)).collect())
    }

    /// 获取收藏
    pub fn get_favorites(&self) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        Ok(all.into_iter().filter(|p| p.favorite).collect())
    }

    /// 获取弱密码
    pub fn get_weak(&self) -> Result<Vec<PasswordDto>, String> {
        let all = self.get_all()?;
        Ok(all.into_iter().filter(|p| p.strength <= 2).collect())
    }

    /// 新增密码
    pub fn insert(&self, input: &PasswordInput) -> Result<PasswordDto, String> {
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        let encrypted = self.crypto.encrypt(&input.password)?;
        let tags_json = serde_json::to_string(
            &input.tags.clone().unwrap_or_default(),
        )
        .map_err(|e| format!("标签序列化失败: {}", e))?;

        let strength = calculate_strength(&input.password);
        let now = chrono::Local::now().format("%Y-%m-%d").to_string();
        let icon = input.icon.clone().unwrap_or_else(|| "🔑".to_string());
        let url = input.url.clone().unwrap_or_default();
        let username = input.username.clone().unwrap_or_default();
        let notes = input.notes.clone().unwrap_or_default();
        let favorite = input.favorite.unwrap_or(false);

        conn.execute(
            "INSERT INTO passwords (name, icon, url, username, password_encrypted, tags, notes, favorite, strength, created, last_used) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                input.name,
                icon,
                url,
                username,
                encrypted,
                tags_json,
                notes,
                favorite as i32,
                strength,
                now,
                now,
            ],
        )
        .map_err(|e| format!("插入失败: {}", e))?;

        let id = conn.last_insert_rowid();
        Ok(PasswordDto {
            id,
            name: input.name.clone(),
            icon,
            url,
            username,
            password: input.password.clone(),
            tags: input.tags.clone().unwrap_or_default(),
            notes,
            favorite,
            strength,
            created: now.clone(),
            last_used: now,
        })
    }

    /// 更新密码
    pub fn update(&self, id: i64, input: &PasswordInput) -> Result<PasswordDto, String> {
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        let encrypted = self.crypto.encrypt(&input.password)?;
        let tags_json = serde_json::to_string(&input.tags.clone().unwrap_or_default())
            .map_err(|e| format!("标签序列化失败: {}", e))?;

        let strength = calculate_strength(&input.password);
        let icon = input.icon.clone().unwrap_or_else(|| "🔑".to_string());
        let url = input.url.clone().unwrap_or_default();
        let username = input.username.clone().unwrap_or_default();
        let notes = input.notes.clone().unwrap_or_default();
        let favorite = input.favorite.unwrap_or(false);

        conn.execute(
            "UPDATE passwords SET name=?1, icon=?2, url=?3, username=?4, password_encrypted=?5, tags=?6, notes=?7, favorite=?8, strength=?9 WHERE id=?10",
            params![
                input.name,
                icon,
                url,
                username,
                encrypted,
                tags_json,
                notes,
                favorite as i32,
                strength,
                id,
            ],
        )
        .map_err(|e| format!("更新失败: {}", e))?;

        Ok(PasswordDto {
            id,
            name: input.name.clone(),
            icon,
            url,
            username,
            password: input.password.clone(),
            tags: input.tags.clone().unwrap_or_default(),
            notes,
            favorite,
            strength,
            created: String::new(),
            last_used: String::new(),
        })
    }

    /// 删除密码
    pub fn delete(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        conn.execute("DELETE FROM passwords WHERE id=?1", params![id])
            .map_err(|e| format!("删除失败: {}", e))?;
        Ok(())
    }

    /// 切换收藏
    pub fn toggle_favorite(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        conn.execute(
            "UPDATE passwords SET favorite = CASE WHEN favorite = 1 THEN 0 ELSE 1 END WHERE id=?1",
            params![id],
        )
        .map_err(|e| format!("更新收藏失败: {}", e))?;
        Ok(())
    }

    /// 更新最后使用时间
    pub fn update_last_used(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        let now = chrono::Local::now().format("%Y-%m-%d").to_string();
        conn.execute(
            "UPDATE passwords SET last_used=?1 WHERE id=?2",
            params![now, id],
        )
        .map_err(|e| format!("更新失败: {}", e))?;
        Ok(())
    }

    /// 从外部 SQLite 文件导入数据（反向转换）
    /// 读取外部 .db 文件中的密码表，导入到当前数据库
    pub fn import_from_db_file(&self, file_path: &Path) -> Result<usize, String> {
        let ext_conn = Connection::open(file_path)
            .map_err(|e| format!("打开外部数据库失败: {}", e))?;

        // 验证表结构
        ext_connection_check(&ext_conn)?;

        let mut stmt = ext_connection_query(&ext_conn)?;

        // 使用当前应用的加密器尝试解密外部数据库中的密码
        // 如果外部数据库由本应用导出（同一 master.key），解密成功
        // 如果外部数据库使用明文存储或其他密钥，解密失败后当作明文处理
        let rows = stmt
            .query_map([], |row| {
                let password_encrypted: String = row.get(5)?;
                Ok((
                    row.get::<_, i64>(0)?,        // id
                    row.get::<_, String>(1)?,      // name
                    row.get::<_, String>(2)?,      // icon
                    row.get::<_, String>(3)?,      // url
                    row.get::<_, String>(4)?,      // username
                    password_encrypted,             // password_encrypted
                    row.get::<_, String>(6)?,      // tags
                    row.get::<_, String>(7)?,      // notes
                    row.get::<_, i32>(8)?,         // favorite
                    row.get::<_, i32>(9)?,         // strength
                    row.get::<_, String>(10)?,     // created
                    row.get::<_, String>(11)?,     // last_used
                ))
            })
            .map_err(|e| format!("读取外部数据失败: {}", e))?;

        let mut count = 0;
        let conn = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;

        for row in rows {
            let (
                _id,
                name,
                icon,
                url,
                username,
                password_encrypted,
                tags_json,
                notes,
                favorite,
                strength,
                created,
                last_used,
            ) = row.map_err(|e| format!("行数据错误: {}", e))?;

            // 尝试用当前应用密钥解密（本应用导出的加密格式），失败则当作明文
            let password_plain = self
                .crypto
                .decrypt(&password_encrypted)
                .unwrap_or(password_encrypted);

            let re_encrypted = self.crypto.encrypt(&password_plain)?;

            conn.execute(
                "INSERT INTO passwords (name, icon, url, username, password_encrypted, tags, notes, favorite, strength, created, last_used) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![name, icon, url, username, re_encrypted, tags_json, notes, favorite, strength, created, last_used],
            )
            .map_err(|e| format!("导入插入失败: {}", e))?;

            count += 1;
        }

        Ok(count)
    }

    /// 将行数据转换为 DTO（含解密）
    fn row_to_dto(&self, row: &rusqlite::Row) -> rusqlite::Result<PasswordDto> {
        let password_encrypted: String = row.get(5)?;
        let tags_json: String = row.get(6)?;

        let password = self
            .crypto
            .decrypt(&password_encrypted)
            .unwrap_or_default();

        let tags: Vec<String> =
            serde_json::from_str(&tags_json).unwrap_or_default();

        Ok(PasswordDto {
            id: row.get(0)?,
            name: row.get(1)?,
            icon: row.get(2)?,
            url: row.get(3)?,
            username: row.get(4)?,
            password,
            tags,
            notes: row.get(7)?,
            favorite: row.get::<_, i32>(8)? != 0,
            strength: row.get(9)?,
            created: row.get(10)?,
            last_used: row.get(11)?,
        })
    }

    /// 关闭当前数据库连接，释放文件句柄
    /// 调用后 `self.conn` 被替换为一个新的内存占位 Connection
    pub fn close(&mut self) {
        // 用内存数据库占位替换原 Connection，原 Connection 在此 drop，释放文件句柄
        if let Ok(mem_conn) = Connection::open_in_memory() {
            let mut guard = self.conn.lock().unwrap();
            let _ = std::mem::replace(&mut *guard, mem_conn);
        }
    }

    /// 执行 WAL checkpoint，将 WAL 文件中的数据合并到主 db 文件
    /// 用于上传前确保主 db 文件包含最新数据（WAL 模式下写入先进 -wal 文件）
    pub fn checkpoint(&self) -> Result<(), String> {
        let guard = self.conn.lock().map_err(|e| format!("锁失败: {}", e))?;
        guard
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| format!("checkpoint 失败: {}", e))?;
        eprintln!("[db] wal_checkpoint TRUNCATE executed");
        Ok(())
    }

    /// 重新打开数据库连接
    /// 用于数据库文件被外部替换后（如从云恢复下载覆盖）重新加载
    /// 步骤：关闭旧 Connection → 删除 WAL/SHM → 重新 open → 建表
    pub fn reopen(&mut self, db_path: &Path) -> Result<(), String> {
        // 1. 关闭旧 Connection（释放文件句柄）
        self.close();

        // 2. 删除残留的 WAL 和 SHM 文件（避免旧 WAL 污染新数据库）
        let wal_path = db_path.with_extension("db-wal");
        let shm_path = db_path.with_extension("db-shm");
        let _ = std::fs::remove_file(&wal_path);
        let _ = std::fs::remove_file(&shm_path);

        // 3. 重新打开数据库
        let conn = Connection::open(db_path)
            .map_err(|e| format!("重新打开数据库失败: {}", e))?;

        // 4. 启用 WAL 模式和 foreign_keys
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("设置数据库参数失败: {}", e))?;

        // 5. 创建表（若不存在）
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS passwords (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                icon TEXT DEFAULT '🔑',
                url TEXT DEFAULT '',
                username TEXT DEFAULT '',
                password_encrypted TEXT NOT NULL,
                tags TEXT DEFAULT '[]',
                notes TEXT DEFAULT '',
                favorite INTEGER DEFAULT 0,
                strength INTEGER DEFAULT 0,
                created TEXT DEFAULT '',
                last_used TEXT DEFAULT ''
            );
            CREATE TABLE IF NOT EXISTS app_config (
                key TEXT PRIMARY KEY,
                value TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_passwords_name ON passwords(name);
            CREATE INDEX IF NOT EXISTS idx_passwords_favorite ON passwords(favorite);",
        )
        .map_err(|e| format!("创建表失败: {}", e))?;

        // 6. 替换为新 Connection（crypto 保持不变）
        let mut guard = self.conn.lock().unwrap();
        let _ = std::mem::replace(&mut *guard, conn);
        Ok(())
    }
}

#[allow(dead_code)]
fn ext_connection_check(conn: &Connection) -> Result<(), String> {
    conn.prepare("SELECT count(*) FROM passwords")
        .map_err(|_| "外部文件不是有效的密码数据库（缺少 passwords 表）".to_string())?;
    Ok(())
}

#[allow(dead_code)]
fn ext_connection_query(conn: &Connection) -> Result<rusqlite::Statement<'_>, String> {
    conn.prepare("SELECT id, name, icon, url, username, password_encrypted, tags, notes, favorite, strength, created, last_used FROM passwords")
        .map_err(|e| format!("准备查询失败: {}", e))
}

/// 检查 SQLite 数据库文件完整性
/// 执行 PRAGMA integrity_check，返回 true 表示数据库有效（结果为 "ok"）
#[allow(dead_code)]
pub fn integrity_check(db_path: &Path) -> bool {
    match Connection::open(db_path) {
        Ok(conn) => {
            match conn.query_row("PRAGMA integrity_check;", [], |row| {
                let result: String = row.get(0)?;
                Ok(result)
            }) {
                Ok(result) => result == "ok",
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}

//! 同步会话的线性化点：取消与凭据提交共用一把锁。
use std::sync::Mutex;

#[derive(Default)]
pub struct SyncSession(Mutex<SessionState>);

/// API Key 操作使用独立会话，不能与网页登录的取消/清理相互作废。
#[derive(Default)]
pub struct ApiKeySession(pub SyncSession);

#[derive(Default)]
struct SessionState {
    generation: u64,
    active: bool,
    clearing: bool,
}

impl SyncSession {
    pub fn begin(&self) -> Result<u64, String> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.clearing {
            return Err("网页登录数据正在清理，请稍后再试".into());
        }
        state.generation += 1;
        state.active = true;
        Ok(state.generation)
    }

    pub fn is_current(&self, generation: u64) -> bool {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.active && state.generation == generation
    }

    pub fn active_generation(&self) -> Option<u64> {
        let state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.active.then_some(state.generation)
    }

    pub fn commit<T>(
        &self,
        generation: u64,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.perform(generation, true, action)
    }

    pub fn with_current<T>(
        &self,
        generation: u64,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        self.perform(generation, false, action)
    }

    fn perform<T>(
        &self,
        generation: u64,
        finish: bool,
        action: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if !state.active || state.generation != generation {
            return Err("同步已取消或已被新的操作替代".to_string());
        }
        let value = action()?;
        if finish {
            state.active = false;
        }
        Ok(value)
    }

    pub fn cancel<T>(&self, action: impl FnOnce() -> T) -> T {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        state.generation += 1;
        state.active = false;
        action()
    }

    pub fn start_clear<T>(&self, action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.clearing {
            return Err("网页登录数据仍在清理，请等待完成".into());
        }
        state.generation += 1;
        state.active = false;
        let value = action()?;
        state.clearing = true;
        Ok(value)
    }

    pub fn finish_clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clearing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key清除拒绝迟到保存_新保存优先_不影响用量同步() {
        let key = ApiKeySession::default();
        let usage = SyncSession::default();
        let usage_generation = usage.begin().unwrap();
        let old = key.0.begin().unwrap();
        let mut stored = Some("original");
        key.0.cancel(|| stored = None);
        assert!(key
            .0
            .commit(old, || {
                stored = Some("late");
                Ok(())
            })
            .is_err());
        assert_eq!(stored, None);
        let first = key.0.begin().unwrap();
        let second = key.0.begin().unwrap();
        key.0
            .commit(second, || {
                stored = Some("new");
                Ok(())
            })
            .unwrap();
        assert!(key
            .0
            .commit(first, || {
                stored = Some("old");
                Ok(())
            })
            .is_err());
        assert_eq!(stored, Some("new"));
        assert!(usage.is_current(usage_generation));
    }

    #[test]
    fn 取消与新会话拒绝旧提交_成功只能提交一次() {
        let session = SyncSession::default();
        let first = session.begin().unwrap();
        session.cancel(|| ());
        assert!(session.commit(first, || Ok(())).is_err());
        let second = session.begin().unwrap();
        let third = session.begin().unwrap();
        assert!(session.commit(second, || Ok(())).is_err());
        assert_eq!(session.commit(third, || Ok("saved")).unwrap(), "saved");
        assert!(session.commit(third, || Ok(())).is_err());
    }

    #[test]
    fn 保存失败允许重试_取消后不执行保存回调() {
        let session = SyncSession::default();
        let generation = session.begin().unwrap();
        assert!(session
            .commit::<()>(generation, || Err("disk".into()))
            .is_err());
        assert!(session.is_current(generation));
        session.cancel(|| ());
        assert!(session
            .commit::<()>(generation, || {
                panic!("过期提交不能运行");
            })
            .is_err());
    }

    #[test]
    fn 清理期间禁止新同步_旧提交失效_完成后可重新登录() {
        let session = SyncSession::default();
        let old = session.begin().unwrap();
        session.start_clear(|| Ok(())).unwrap();
        assert!(session.begin().is_err());
        assert!(session.commit(old, || Ok(())).is_err());
        session.finish_clear();
        assert!(session.begin().is_ok());
    }
}

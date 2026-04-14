//! PoW 计算器 —— 基于 DeepSeek WASM 的 DeepSeekHashV1 算法实现
//!
//! alloc 导出符号名（__wbindgen_export_N）在启动时动态探测，不依赖硬编码名称。

use wasmtime::{AsContextMut, Engine, ExternType, InstancePre, Linker, Module, Store, ValType};

// 复用 client 的 ChallengeData，避免重复定义
pub use crate::ds_core::client::ChallengeData as Challenge;

#[derive(Clone)]
pub struct PowSolver {
    engine: Engine,
    instance_pre: InstancePre<()>,
    alloc_fn: String,
}

#[derive(Debug)]
pub struct PowResult {
    pub algorithm: String,
    pub challenge: String,
    pub salt: String,
    pub answer: i64,
    pub signature: String,
    pub target_path: String,
}

impl PowResult {
    /// 将 PoW 结果转换为 base64 编码的 header
    pub fn to_header(&self) -> String {
        let json = serde_json::json!({
            "algorithm": self.algorithm,
            "challenge": self.challenge,
            "salt": self.salt,
            "answer": self.answer,
            "signature": self.signature,
            "target_path": self.target_path,
        });
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            json.to_string().as_bytes(),
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PowError {
    #[error("WASM init failed: {0}")]
    WasmInit(String),
    #[error("WASM solve failed: no solution")]
    NoSolution,
    #[error("unsupported algorithm: {0}")]
    UnsupportedAlgorithm(String),
    #[error("WASM execution error: {0}")]
    Execution(String),
}

impl PowSolver {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, PowError> {
        let engine = Engine::default();
        let module =
            Module::new(&engine, wasm_bytes).map_err(|e| PowError::WasmInit(e.to_string()))?;
        let alloc_fn = find_alloc_export(&module)?;
        let linker = Linker::new(&engine);
        let instance_pre = linker
            .instantiate_pre(&module)
            .map_err(|e| PowError::WasmInit(e.to_string()))?;
        Ok(Self {
            engine,
            instance_pre,
            alloc_fn,
        })
    }

    pub fn solve(&self, challenge: &Challenge) -> Result<PowResult, PowError> {
        if challenge.algorithm != "DeepSeekHashV1" {
            return Err(PowError::UnsupportedAlgorithm(challenge.algorithm.clone()));
        }

        let mut store = Store::new(&self.engine, ());

        let instance = self
            .instance_pre
            .instantiate(&mut store)
            .map_err(|e| PowError::Execution(e.to_string()))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| PowError::Execution("memory not found".to_string()))?;
        let add_to_stack = instance
            .get_typed_func::<i32, i32>(&mut store, "__wbindgen_add_to_stack_pointer")
            .map_err(|e| PowError::Execution(e.to_string()))?;
        let alloc = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, &self.alloc_fn)
            .map_err(|e| PowError::Execution(e.to_string()))?;
        let wasm_solve = instance
            .get_typed_func::<(i32, i32, i32, i32, i32, f64), ()>(&mut store, "wasm_solve")
            .map_err(|e| PowError::Execution(e.to_string()))?;

        let prefix = format!("{}_{}_", challenge.salt, challenge.expire_at);
        let retptr = add_to_stack
            .call(&mut store, -16)
            .map_err(|e| PowError::Execution(e.to_string()))?;

        let (ptr_challenge, len_challenge) =
            write_string(&mut store, &memory, &alloc, &challenge.challenge)?;
        let (ptr_prefix, len_prefix) = write_string(&mut store, &memory, &alloc, &prefix)?;

        wasm_solve
            .call(
                &mut store,
                (
                    retptr,
                    ptr_challenge,
                    len_challenge,
                    ptr_prefix,
                    len_prefix,
                    challenge.difficulty as f64,
                ),
            )
            .map_err(|e| PowError::Execution(e.to_string()))?;

        let mut status_buf = [0u8; 4];
        memory
            .read(&mut store, retptr as usize, &mut status_buf)
            .map_err(|e| PowError::Execution(e.to_string()))?;
        let status = i32::from_le_bytes(status_buf);

        let mut value_buf = [0u8; 8];
        memory
            .read(&mut store, (retptr + 8) as usize, &mut value_buf)
            .map_err(|e| PowError::Execution(e.to_string()))?;
        let value = f64::from_le_bytes(value_buf);

        add_to_stack
            .call(&mut store, 16)
            .map_err(|e| PowError::Execution(e.to_string()))?;

        if status == 0 {
            return Err(PowError::NoSolution);
        }

        Ok(PowResult {
            algorithm: challenge.algorithm.clone(),
            challenge: challenge.challenge.clone(),
            salt: challenge.salt.clone(),
            answer: value as i64,
            signature: challenge.signature.clone(),
            target_path: challenge.target_path.clone(),
        })
    }
}

fn write_string(
    store: &mut Store<()>,
    memory: &wasmtime::Memory,
    alloc: &wasmtime::TypedFunc<(i32, i32), i32>,
    text: &str,
) -> Result<(i32, i32), PowError> {
    let bytes = text.as_bytes();
    let len = bytes.len() as i32;
    let ptr = alloc
        .call(store.as_context_mut(), (len, 1))
        .map_err(|e| PowError::Execution(e.to_string()))?;
    memory
        .write(store.as_context_mut(), ptr as usize, bytes)
        .map_err(|e| PowError::Execution(e.to_string()))?;
    Ok((ptr, len))
}

/// 在 WASM 导出中动态查找 alloc 函数（签名：(i32, i32) -> i32，名称前缀 __wbindgen_export_）。
/// 避免硬编码 __wbindgen_export_0，防止 DeepSeek 重新编译 WASM 后符号名变更导致启动失败。
fn find_alloc_export(module: &Module) -> Result<String, PowError> {
    let i32_i32_to_i32 = |ft: &wasmtime::FuncType| {
        let mut p = ft.params();
        let mut r = ft.results();
        matches!(p.next(), Some(ValType::I32))
            && matches!(p.next(), Some(ValType::I32))
            && p.next().is_none()
            && matches!(r.next(), Some(ValType::I32))
            && r.next().is_none()
    };

    module
        .exports()
        .find(|e| {
            e.name().starts_with("__wbindgen_export_")
                && matches!(e.ty(), ExternType::Func(ft) if i32_i32_to_i32(&ft))
        })
        .map(|e| e.name().to_string())
        .ok_or_else(|| {
            PowError::WasmInit(
                "找不到 alloc 导出（__wbindgen_export_* with (i32,i32)->i32）".to_string(),
            )
        })
}

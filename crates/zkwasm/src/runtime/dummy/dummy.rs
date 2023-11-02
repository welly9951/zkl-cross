use wasmi::{Externals, FuncInstance, TrapCode};
use wasmi::ModuleImportResolver;
use wasmi::RuntimeArgs;
use wasmi::RuntimeValue;
use wasmi::Trap;

pub struct Dummy {}

impl Dummy {
    pub fn new() -> Self {
        Self{}
    }
}

impl ModuleImportResolver for Dummy {
    fn resolve_func(
        &self,
        _function_name: &str,
        signature: &wasmi::Signature,
    ) -> Result<wasmi::FuncRef, wasmi::Error> {
        Ok(FuncInstance::alloc_host(
            signature.clone(),
            0, // dummy one
        ))
    }
}

impl Externals for Dummy {
    fn invoke_index(
        &mut self,
        _index: usize,
        _args: RuntimeArgs,
    ) -> Result<Option<RuntimeValue>, Trap> {
        Err(TrapCode::Unreachable.into())
    }
}

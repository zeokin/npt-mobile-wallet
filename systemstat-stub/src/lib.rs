/// Stub implementation — mobile wallet doesn't use system stats.
/// neptune-cash imports this transitively for the RPC server (which
/// the mobile wallet doesn't run).

pub struct System;

impl System {
    pub fn new() -> Self {
        System
    }
}

impl Default for System {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Platform {
    fn cpu_load_aggregate(&self) -> Result<DelayedMeasurement<CPULoad>, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not supported on this platform",
        ))
    }
    fn cpu_temp(&self) -> Result<f32, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not supported on this platform",
        ))
    }
    fn memory(&self) -> Result<Memory, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not supported on this platform",
        ))
    }
}

impl Platform for System {}

pub struct DelayedMeasurement<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> DelayedMeasurement<T> {
    #[expect(unused)]
    pub(crate) fn done(&self) -> Result<T, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "not supported on this platform",
        ))
    }
}

#[expect(unused)]
#[derive(Debug, Clone)]
pub struct CPULoad {
    pub(crate) user: f32,
    pub(crate) nice: f32,
    pub(crate) system: f32,
    pub(crate) interrupt: f32,
    pub(crate) idle: f32,
}

#[expect(unused)]
#[derive(Debug, Clone)]
pub struct Memory {
    pub(crate) total: ByteSize,
    pub(crate) free: ByteSize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ByteSize(pub(crate) u64);

impl std::fmt::Display for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} B", self.0)
    }
}

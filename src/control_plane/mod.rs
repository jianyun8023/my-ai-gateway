pub(crate) mod model_catalog;
pub(crate) mod model_discovery;

mod accounts;
mod bindings;
mod error;
mod import;
mod models;
mod repository;
mod routes;
mod service;
mod snapshot;
mod sources;
mod types;
mod validation;

pub(crate) use error::ControlPlaneError;
pub(crate) use service::ControlPlane;
pub(crate) use snapshot::RuntimeSnapshot;
pub(crate) use types::{
    AccountWrite, EnabledWrite, LogicalModelWrite, ModelBindingWrite, Mutation, RouteWrite,
    SourceCreateWrite, SourceWrite,
};

#[cfg(test)]
mod tests;

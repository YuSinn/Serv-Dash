mod commands;
mod error;
mod model;
mod node;
mod scanner;
mod windows_scripts;

pub(crate) use commands::detect_project_services;

#[cfg(test)]
mod detection_test_support;
#[cfg(test)]
mod tests;

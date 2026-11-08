//! # forja-syntax
//!
//! Frontend reutilizable del lenguaje **Forja (fa)**: tokens, AST, lexer,
//! parser y el sistema de diagnósticos (`ErrorForja`).
//!
//! Este crate no depende del backend (bytecode, VM, JIT, codegen) ni de la GUI,
//! de modo que herramientas de terceros (linters, formateadores, editores,
//! servidores LSP) puedan consumir el frontend sin arrastrar el compilador
//! completo.
//!
//! El crate `forja` reexporta estos módulos (`forja::ast`, `forja::lexer`, …)
//! para preservar la API pública histórica.
#![forbid(unsafe_code)]
// `ErrorForja` es rico a propósito (código, mensaje, ayuda y dos spans). Boxearlo
// en cada firma es una migración aparte; hasta entonces se permite el tamaño.
#![allow(clippy::result_large_err)]
#![allow(rustdoc::broken_intra_doc_links)]

pub mod ast;
pub mod booleanos;
pub use forja_errors as error;
pub mod lexer;
pub mod parser;
pub mod token;

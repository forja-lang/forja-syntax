//! `v` y `f` como alias de `verdadero` y `falso`.
//!
//! No son palabras reservadas: `v` y `f` siguen siendo identificadores válidos
//! (hay programas con `variable v = ...`). Se convierten en literales
//! booleanos **sólo** cuando el nombre no está declarado en ninguna parte del
//! programa (variable, constante, parámetro, campo, variable de patrón…).
//!
//! Como antes un `v`/`f` sin declarar era un error de compilación, ningún
//! programa que ya compilaba cambia de significado.

use std::collections::HashSet;

use crate::ast::*;

/// Reescribe `v`/`f` no declarados como `verdadero`/`falso`.
pub fn normalizar(programa: &mut Programa) {
    let mut declarados = HashSet::new();
    recolectar_declaraciones(&programa.declaraciones, &mut declarados);

    // Sin ninguna declaración de `v`/`f`, el recorrido igual hace falta para
    // llegar a las expresiones anidadas.
    for declaracion in &mut programa.declaraciones {
        reescribir_declaracion(declaracion, &declarados);
    }
}

// ─── Recolección de nombres declarados ─────────────────────────────────────

fn recolectar_declaraciones(declaraciones: &[Declaracion], nombres: &mut HashSet<String>) {
    for declaracion in declaraciones {
        recolectar_declaracion(declaracion, nombres);
    }
}

fn recolectar_declaracion(declaracion: &Declaracion, nombres: &mut HashSet<String>) {
    match declaracion {
        Declaracion::Variable { nombre, valor, .. } => {
            nombres.insert(nombre.clone());
            if let Some(v) = valor {
                recolectar_expr(v, nombres);
            }
        }
        Declaracion::Asignacion { valor, .. } => recolectar_expr(valor, nombres),
        Declaracion::AsignacionMiembro { objeto, valor, .. } => {
            recolectar_expr(objeto, nombres);
            recolectar_expr(valor, nombres);
        }
        Declaracion::AsignacionIndex { indice, valor, .. } => {
            recolectar_expr(indice, nombres);
            recolectar_expr(valor, nombres);
        }
        Declaracion::Funcion {
            nombre,
            parametros,
            cuerpo,
            precondiciones,
            postcondiciones,
            ..
        } => {
            nombres.insert(nombre.clone());
            for p in parametros {
                nombres.insert(p.nombre.clone());
            }
            for c in precondiciones.iter().chain(postcondiciones) {
                recolectar_expr(&c.condicion, nombres);
            }
            recolectar_declaraciones(cuerpo, nombres);
        }
        Declaracion::Clase {
            nombre,
            campos,
            metodos,
            invariantes,
            ..
        } => {
            nombres.insert(nombre.clone());
            for campo in campos {
                nombres.insert(campo.nombre.clone());
            }
            for m in metodos {
                recolectar_metodo(m, nombres);
            }
            for c in invariantes {
                recolectar_expr(&c.condicion, nombres);
            }
        }
        Declaracion::Si {
            condicion,
            bloque_verdadero,
            bloque_falso,
        } => {
            recolectar_expr(condicion, nombres);
            recolectar_declaraciones(bloque_verdadero, nombres);
            if let Some(bloque) = bloque_falso {
                recolectar_declaraciones(bloque, nombres);
            }
        }
        Declaracion::Mientras { condicion, bloque } => {
            recolectar_expr(condicion, nombres);
            recolectar_declaraciones(bloque, nombres);
        }
        Declaracion::Para {
            inicializacion,
            condicion,
            incremento,
            bloque,
        } => {
            if let Some(init) = inicializacion {
                recolectar_declaracion(init, nombres);
            }
            if let Some(cond) = condicion {
                recolectar_expr(cond, nombres);
            }
            if let Some(inc) = incremento {
                recolectar_declaracion(inc, nombres);
            }
            recolectar_declaraciones(bloque, nombres);
        }
        Declaracion::Repetir { cantidad, bloque } => {
            recolectar_expr(cantidad, nombres);
            recolectar_declaraciones(bloque, nombres);
        }
        Declaracion::Cuando {
            condicion, cuerpo, ..
        } => {
            recolectar_expr(condicion, nombres);
            recolectar_declaraciones(cuerpo, nombres);
        }
        Declaracion::LlamadaFuncion { argumentos, .. } => {
            for a in argumentos {
                recolectar_expr(a, nombres);
            }
        }
        Declaracion::AccesoMiembro { objeto, .. } => recolectar_expr(objeto, nombres),
        Declaracion::Retornar { valor } => {
            if let Some(v) = valor {
                recolectar_expr(v, nombres);
            }
        }
        Declaracion::Enum {
            nombre, variantes, ..
        } => {
            nombres.insert(nombre.clone());
            for v in variantes {
                nombres.insert(v.nombre.clone());
            }
        }
        Declaracion::AsignacionMultiple {
            variables, valor, ..
        } => {
            for v in variables {
                nombres.insert(v.clone());
            }
            recolectar_expr(valor, nombres);
        }
        Declaracion::Rasgo {
            nombre, metodos, ..
        } => {
            nombres.insert(nombre.clone());
            for m in metodos {
                for p in &m.parametros {
                    nombres.insert(p.nombre.clone());
                }
            }
        }
        Declaracion::Implementacion {
            rasgo_nombre,
            clase_nombre,
            metodos,
        } => {
            nombres.insert(rasgo_nombre.clone());
            nombres.insert(clase_nombre.clone());
            for m in metodos {
                recolectar_metodo(m, nombres);
            }
        }
        Declaracion::Expresion(e) => recolectar_expr(e, nombres),
        Declaracion::Romper
        | Declaracion::Continuar
        | Declaracion::Importar(_)
        | Declaracion::ImportarExterna(_) => {}
    }
}

fn recolectar_metodo(metodo: &Metodo, nombres: &mut HashSet<String>) {
    nombres.insert(metodo.nombre.clone());
    for p in &metodo.parametros {
        nombres.insert(p.nombre.clone());
    }
    for c in metodo.precondiciones.iter().chain(&metodo.postcondiciones) {
        recolectar_expr(&c.condicion, nombres);
    }
    recolectar_declaraciones(&metodo.cuerpo, nombres);
}

fn recolectar_patron(patron: &Patron, nombres: &mut HashSet<String>) {
    match patron {
        Patron::Variable(nombre) => {
            nombres.insert(nombre.clone());
        }
        Patron::Literal(e) => recolectar_expr(e, nombres),
        Patron::Constructor(_, subpatrones) => {
            for p in subpatrones {
                recolectar_patron(p, nombres);
            }
        }
        Patron::Ignorar => {}
    }
}

fn recolectar_expr(expr: &Expresion, nombres: &mut HashSet<String>) {
    match expr {
        Expresion::LiteralNumero(_)
        | Expresion::LiteralDecimal(_)
        | Expresion::LiteralTexto(_)
        | Expresion::LiteralBooleano(_)
        | Expresion::LiteralNulo
        | Expresion::LiteralExacto(_, _)
        | Expresion::Identificador { .. }
        | Expresion::Nada
        | Expresion::Ninguno
        | Expresion::Resultado => {}
        Expresion::Binaria {
            izquierda, derecha, ..
        } => {
            recolectar_expr(izquierda, nombres);
            recolectar_expr(derecha, nombres);
        }
        Expresion::Unaria { expr, .. } | Expresion::Grupo(expr) | Expresion::Try(expr) => {
            recolectar_expr(expr, nombres);
        }
        Expresion::Ternario {
            condicion,
            si_verdadero,
            si_falso,
        } => {
            recolectar_expr(condicion, nombres);
            recolectar_expr(si_verdadero, nombres);
            recolectar_expr(si_falso, nombres);
        }
        Expresion::LlamadaFuncion { argumentos, .. }
        | Expresion::Instanciacion { argumentos, .. } => {
            for a in argumentos {
                recolectar_expr(a, nombres);
            }
        }
        Expresion::LlamadaMetodo {
            objeto, argumentos, ..
        } => {
            recolectar_expr(objeto, nombres);
            for a in argumentos {
                recolectar_expr(a, nombres);
            }
        }
        Expresion::AccesoMiembro { objeto, .. }
        | Expresion::Referencia { expr: objeto, .. }
        | Expresion::Anterior(objeto) => recolectar_expr(objeto, nombres),
        Expresion::Arreglo(elementos) => {
            for e in elementos {
                recolectar_expr(e, nombres);
            }
        }
        Expresion::Mapa(pares) => {
            for (k, v) in pares {
                recolectar_expr(k, nombres);
                recolectar_expr(v, nombres);
            }
        }
        Expresion::Coincidir { expr, brazos } => {
            recolectar_expr(expr, nombres);
            for brazo in brazos {
                recolectar_patron(&brazo.patron, nombres);
                recolectar_declaraciones(&brazo.cuerpo, nombres);
            }
        }
        Expresion::Index { objeto, indice } => {
            recolectar_expr(objeto, nombres);
            recolectar_expr(indice, nombres);
        }
        Expresion::Closure {
            parametros, cuerpo, ..
        } => {
            for p in parametros {
                nombres.insert(p.nombre.clone());
            }
            recolectar_declaraciones(cuerpo, nombres);
        }
        Expresion::Si {
            condicion,
            bloque_verdadero,
            bloque_falso,
        } => {
            recolectar_expr(condicion, nombres);
            recolectar_declaraciones(bloque_verdadero, nombres);
            if let Some(b) = bloque_falso {
                recolectar_declaraciones(b, nombres);
            }
        }
        Expresion::Hilo { cuerpo } => recolectar_declaraciones(cuerpo, nombres),
        Expresion::CanalNuevo(capacidad) => {
            if let Some(c) = capacidad {
                recolectar_expr(c, nombres);
            }
        }
        Expresion::Seleccionar { brazos } => {
            for brazo in brazos {
                if let Some((variable, expr)) = &brazo.recepcion {
                    nombres.insert(variable.clone());
                    recolectar_expr(expr, nombres);
                }
                recolectar_declaraciones(&brazo.cuerpo, nombres);
            }
        }
        Expresion::Asignacion { valor, .. } => recolectar_expr(valor, nombres),
        Expresion::AsignacionCampo { objeto, valor, .. } => {
            recolectar_expr(objeto, nombres);
            recolectar_expr(valor, nombres);
        }
        Expresion::ArraySet { array, valor } => {
            recolectar_expr(array, nombres);
            recolectar_expr(valor, nombres);
        }
        Expresion::AsignacionCompuesta {
            objetivo, valor, ..
        } => {
            recolectar_expr(objetivo, nombres);
            recolectar_expr(valor, nombres);
        }
        Expresion::Ok(e) | Expresion::Error(e) | Expresion::Algo(e) => {
            recolectar_expr(e, nombres);
        }
    }
}

// ─── Reescritura ───────────────────────────────────────────────────────────

fn reescribir_declaraciones(declaraciones: &mut [Declaracion], nombres: &HashSet<String>) {
    for d in declaraciones {
        reescribir_declaracion(d, nombres);
    }
}

fn reescribir_declaracion(declaracion: &mut Declaracion, nombres: &HashSet<String>) {
    match declaracion {
        Declaracion::Variable { valor, .. } => {
            if let Some(v) = valor {
                reescribir_expr(v, nombres);
            }
        }
        Declaracion::Asignacion { valor, .. } => reescribir_expr(valor, nombres),
        Declaracion::AsignacionMiembro { objeto, valor, .. } => {
            reescribir_expr(objeto, nombres);
            reescribir_expr(valor, nombres);
        }
        Declaracion::AsignacionIndex { indice, valor, .. } => {
            reescribir_expr(indice, nombres);
            reescribir_expr(valor, nombres);
        }
        Declaracion::Funcion {
            parametros: _,
            cuerpo,
            precondiciones,
            postcondiciones,
            ..
        } => {
            for c in precondiciones.iter_mut().chain(postcondiciones) {
                reescribir_expr(&mut c.condicion, nombres);
            }
            reescribir_declaraciones(cuerpo, nombres);
        }
        Declaracion::Clase {
            metodos,
            invariantes,
            ..
        } => {
            for m in metodos {
                reescribir_metodo(m, nombres);
            }
            for c in invariantes {
                reescribir_expr(&mut c.condicion, nombres);
            }
        }
        Declaracion::Si {
            condicion,
            bloque_verdadero,
            bloque_falso,
        } => {
            reescribir_expr(condicion, nombres);
            reescribir_declaraciones(bloque_verdadero, nombres);
            if let Some(b) = bloque_falso {
                reescribir_declaraciones(b, nombres);
            }
        }
        Declaracion::Mientras { condicion, bloque } => {
            reescribir_expr(condicion, nombres);
            reescribir_declaraciones(bloque, nombres);
        }
        Declaracion::Para {
            inicializacion,
            condicion,
            incremento,
            bloque,
        } => {
            if let Some(init) = inicializacion {
                reescribir_declaracion(init, nombres);
            }
            if let Some(cond) = condicion {
                reescribir_expr(cond, nombres);
            }
            if let Some(inc) = incremento {
                reescribir_declaracion(inc, nombres);
            }
            reescribir_declaraciones(bloque, nombres);
        }
        Declaracion::Repetir { cantidad, bloque } => {
            reescribir_expr(cantidad, nombres);
            reescribir_declaraciones(bloque, nombres);
        }
        Declaracion::Cuando {
            condicion, cuerpo, ..
        } => {
            reescribir_expr(condicion, nombres);
            reescribir_declaraciones(cuerpo, nombres);
        }
        Declaracion::LlamadaFuncion { argumentos, .. } => {
            for a in argumentos {
                reescribir_expr(a, nombres);
            }
        }
        Declaracion::AccesoMiembro { objeto, .. } => reescribir_expr(objeto, nombres),
        Declaracion::Retornar { valor } => {
            if let Some(v) = valor {
                reescribir_expr(v, nombres);
            }
        }
        Declaracion::AsignacionMultiple { valor, .. } => reescribir_expr(valor, nombres),
        Declaracion::Implementacion { metodos, .. } => {
            for m in metodos {
                reescribir_metodo(m, nombres);
            }
        }
        Declaracion::Expresion(e) => reescribir_expr(e, nombres),
        Declaracion::Romper
        | Declaracion::Continuar
        | Declaracion::Importar(_)
        | Declaracion::ImportarExterna(_)
        | Declaracion::Enum { .. }
        | Declaracion::Rasgo { .. } => {}
    }
}

fn reescribir_metodo(metodo: &mut Metodo, nombres: &HashSet<String>) {
    for c in metodo
        .precondiciones
        .iter_mut()
        .chain(&mut metodo.postcondiciones)
    {
        reescribir_expr(&mut c.condicion, nombres);
    }
    reescribir_declaraciones(&mut metodo.cuerpo, nombres);
}

fn reescribir_patron(patron: &mut Patron, nombres: &HashSet<String>) {
    match patron {
        Patron::Literal(e) => reescribir_expr(e, nombres),
        Patron::Constructor(_, subpatrones) => {
            for p in subpatrones {
                reescribir_patron(p, nombres);
            }
        }
        Patron::Variable(_) | Patron::Ignorar => {}
    }
}

fn reescribir_expr(expr: &mut Expresion, nombres: &HashSet<String>) {
    if let Expresion::Identificador { nombre, .. } = expr {
        let literal = match nombre.as_str() {
            "v" if !nombres.contains("v") => Some(true),
            "f" if !nombres.contains("f") => Some(false),
            _ => None,
        };
        if let Some(valor) = literal {
            *expr = Expresion::LiteralBooleano(valor);
            return;
        }
    }

    match expr {
        Expresion::LiteralNumero(_)
        | Expresion::LiteralDecimal(_)
        | Expresion::LiteralTexto(_)
        | Expresion::LiteralBooleano(_)
        | Expresion::LiteralNulo
        | Expresion::LiteralExacto(_, _)
        | Expresion::Identificador { .. }
        | Expresion::Nada
        | Expresion::Ninguno
        | Expresion::Resultado => {}
        Expresion::Binaria {
            izquierda, derecha, ..
        } => {
            reescribir_expr(izquierda, nombres);
            reescribir_expr(derecha, nombres);
        }
        Expresion::Unaria { expr, .. } | Expresion::Grupo(expr) | Expresion::Try(expr) => {
            reescribir_expr(expr, nombres);
        }
        Expresion::Ternario {
            condicion,
            si_verdadero,
            si_falso,
        } => {
            reescribir_expr(condicion, nombres);
            reescribir_expr(si_verdadero, nombres);
            reescribir_expr(si_falso, nombres);
        }
        Expresion::LlamadaFuncion { argumentos, .. }
        | Expresion::Instanciacion { argumentos, .. } => {
            for a in argumentos {
                reescribir_expr(a, nombres);
            }
        }
        Expresion::LlamadaMetodo {
            objeto, argumentos, ..
        } => {
            reescribir_expr(objeto, nombres);
            for a in argumentos {
                reescribir_expr(a, nombres);
            }
        }
        Expresion::AccesoMiembro { objeto, .. }
        | Expresion::Referencia { expr: objeto, .. }
        | Expresion::Anterior(objeto) => reescribir_expr(objeto, nombres),
        Expresion::Arreglo(elementos) => {
            for e in elementos {
                reescribir_expr(e, nombres);
            }
        }
        Expresion::Mapa(pares) => {
            for (k, v) in pares {
                reescribir_expr(k, nombres);
                reescribir_expr(v, nombres);
            }
        }
        Expresion::Coincidir { expr, brazos } => {
            reescribir_expr(expr, nombres);
            for brazo in brazos {
                reescribir_patron(&mut brazo.patron, nombres);
                reescribir_declaraciones(&mut brazo.cuerpo, nombres);
            }
        }
        Expresion::Index { objeto, indice } => {
            reescribir_expr(objeto, nombres);
            reescribir_expr(indice, nombres);
        }
        Expresion::Closure {
            parametros: _,
            cuerpo,
        } => reescribir_declaraciones(cuerpo, nombres),
        Expresion::Si {
            condicion,
            bloque_verdadero,
            bloque_falso,
        } => {
            reescribir_expr(condicion, nombres);
            reescribir_declaraciones(bloque_verdadero, nombres);
            if let Some(b) = bloque_falso {
                reescribir_declaraciones(b, nombres);
            }
        }
        Expresion::Hilo { cuerpo } => reescribir_declaraciones(cuerpo, nombres),
        Expresion::CanalNuevo(capacidad) => {
            if let Some(c) = capacidad {
                reescribir_expr(c, nombres);
            }
        }
        Expresion::Seleccionar { brazos } => {
            for brazo in brazos {
                if let Some((_, expr)) = &mut brazo.recepcion {
                    reescribir_expr(expr, nombres);
                }
                reescribir_declaraciones(&mut brazo.cuerpo, nombres);
            }
        }
        Expresion::Asignacion { valor, .. } => reescribir_expr(valor, nombres),
        Expresion::AsignacionCampo { objeto, valor, .. } => {
            reescribir_expr(objeto, nombres);
            reescribir_expr(valor, nombres);
        }
        Expresion::ArraySet { array, valor } => {
            reescribir_expr(array, nombres);
            reescribir_expr(valor, nombres);
        }
        Expresion::AsignacionCompuesta {
            objetivo, valor, ..
        } => {
            reescribir_expr(objetivo, nombres);
            reescribir_expr(valor, nombres);
        }
        Expresion::Ok(e) | Expresion::Error(e) | Expresion::Algo(e) => {
            reescribir_expr(e, nombres);
        }
    }
}

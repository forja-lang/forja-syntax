use super::*;

impl Parser {
    /// si (<cond>) { <bloque> } [ sino { <bloque> } ]
    /// si <cond> { <bloque> } [ sino si <cond> { <bloque> } ] [ sino { <bloque> } ]
    pub(crate) fn parsear_si(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'si'
        self.parsear_si_desde_condicion()
    }

    /// Parsea un `si`/`sino si` desde la condición (sin consumir 'si').
    /// Soporta tanto `si (cond)` con paréntesis como `si cond` sin paréntesis.
    /// `si (cond) { … } sino { … }` como **expresión**: se usa como elemento de
    /// una lista de argumentos (`columna(a, si (x) { b } sino { c })`), donde lo
    /// que produce son elementos de UI y no un valor. El token actual es `si`.
    pub(crate) fn parsear_si_expresion(&mut self) -> Result<Expresion, ErrorForja> {
        self.avanzar(); // consume 'si'
        self.parsear_si_expresion_desde_condicion()
    }

    /// Igual que [`Self::parsear_si_expresion`] pero sin consumir el `si` inicial,
    /// para poder encadenar `sino si`.
    pub(crate) fn parsear_si_expresion_desde_condicion(&mut self) -> Result<Expresion, ErrorForja> {
        let condicion = if self.coincide(TokenKind::ParenAbrir) {
            self.avanzar();
            let cond = self.parsear_expresion()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de la condición.",
            )?;
            cond
        } else {
            self.parsear_expresion()?
        };

        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el bloque del 'si'.",
        )?;
        let bloque_verdadero = self.parsear_bloque()?;

        let bloque_falso = if self.coincide(TokenKind::Sino) {
            self.avanzar(); // consume 'sino'
            if self.coincide(TokenKind::Si) {
                let anidado = self.parsear_si_expresion()?;
                Some(vec![Declaracion::Expresion(anidado)])
            } else {
                self.esperar(
                    TokenKind::LlaveAbrir,
                    "Se esperaba '{' para el bloque del 'sino'.",
                )?;
                Some(self.parsear_bloque()?)
            }
        } else if self.coincide_operador_palabra("o")
            && self.peek_siguiente().map(|t| &t.kind) == Some(&TokenKind::Si)
        {
            self.avanzar(); // consume 'o'
            let anidado = self.parsear_si_expresion()?;
            Some(vec![Declaracion::Expresion(anidado)])
        } else {
            None
        };

        Ok(Expresion::Si {
            condicion: Box::new(condicion),
            bloque_verdadero,
            bloque_falso,
        })
    }

    pub(crate) fn parsear_si_desde_condicion(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        // Parse condition — puede llevar paréntesis o no
        let condicion = if self.coincide(TokenKind::ParenAbrir) {
            self.avanzar();
            let cond = self.parsear_expresion()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de la condición.",
            )?;
            cond
        } else {
            // Sin paréntesis: la expresión termina donde aparece '{'
            self.parsear_expresion()?
        };

        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el bloque del 'si'.",
        )?;
        let bloque_verdadero = self.parsear_bloque()?;

        // Manejar sino / o si / sino si
        let bloque_falso = if self.coincide(TokenKind::Sino) {
            self.avanzar(); // consume 'sino'
            if self.coincide(TokenKind::Si) {
                // sino si → otro condicional anidado (else if)
                self.avanzar(); // consume 'si'
                let sino_si = self.parsear_si_desde_condicion()?;
                sino_si.map(|decl| vec![decl])
            } else {
                self.esperar(
                    TokenKind::LlaveAbrir,
                    "Se esperaba '{' para el bloque del 'sino'.",
                )?;
                Some(self.parsear_bloque()?)
            }
        } else if self.coincide_operador_palabra("o")
            && self.peek_siguiente().map(|t| &t.kind) == Some(&TokenKind::Si)
        {
            self.avanzar(); // consume 'o'
            self.avanzar(); // consume 'si'
            let o_si = self.parsear_si_desde_condicion()?;
            o_si.map(|decl| vec![decl])
        } else {
            None
        };

        Ok(Some(Declaracion::Si {
            condicion: Box::new(condicion),
            bloque_verdadero,
            bloque_falso,
        }))
    }

    /// mientras (<cond>) { <bloque> }
    pub(crate) fn parsear_mientras(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'mientras'
        self.esperar(
            TokenKind::ParenAbrir,
            "Se esperaba '(' después de 'mientras'.",
        )?;
        let condicion = self.parsear_expresion()?;
        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después de la condición.",
        )?;
        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el bloque del 'mientras'.",
        )?;
        let bloque = self.parsear_bloque()?;

        Ok(Some(Declaracion::Mientras {
            condicion: Box::new(condicion),
            bloque,
        }))
    }

    /// cuando (<cond>) { <bloque> }
    pub(crate) fn parsear_cuando(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        let (linea, columna) = (self.linea_actual(), self.columna_actual());
        self.avanzar(); // consume 'cuando'
        self.esperar(
            TokenKind::ParenAbrir,
            "Se esperaba '(' después de 'cuando'.",
        )?;
        let condicion = self.parsear_expresion()?;
        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después de la condición.",
        )?;
        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el bloque del 'cuando'.",
        )?;
        let cuerpo = self.parsear_bloque()?;

        Ok(Some(Declaracion::Cuando {
            condicion: Box::new(condicion),
            cuerpo,
            linea,
            columna,
        }))
    }

    /// para (<inicio>; <cond>; <incr>) { <bloque> }
    pub(crate) fn parsear_para(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'para'
        self.esperar(TokenKind::ParenAbrir, "Se esperaba '(' después de 'para'.")?;

        // Inicialización (puede ser variable decl o asignación)
        let inicializacion = if self.coincide(TokenKind::PuntoComa) {
            None
        } else if self.coincide(TokenKind::Variable) {
            let decl = self.parsear_declaracion_variable()?.unwrap();
            Some(Box::new(decl))
        } else {
            let (linea, columna) = (self.linea_actual(), self.columna_actual());
            let nombre = self.esperar_identificador(
                "Se esperaba una variable en la inicialización del 'para'.",
            )?;
            if self.coincide(TokenKind::Igual) {
                self.avanzar();
                let valor = self.parsear_expresion()?;
                Some(Box::new(Declaracion::Asignacion {
                    nombre,
                    valor: Box::new(valor),
                    linea,
                    columna,
                }))
            } else {
                return Err(ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    self.columna_actual(),
                    Span::punto(self.columna_actual()),
                    "Se esperaba '=' en la inicialización del 'para'.",
                    "Usá 'variable mut i = 0' o 'i = 0' como inicialización.",
                ));
            }
        };

        self.esperar(
            TokenKind::PuntoComa,
            "Se esperaba ';' después de la inicialización.",
        )?;

        // Condición
        let condicion = if self.coincide(TokenKind::PuntoComa) {
            None
        } else {
            Some(Box::new(self.parsear_expresion()?))
        };

        self.esperar(
            TokenKind::PuntoComa,
            "Se esperaba ';' después de la condición.",
        )?;

        // Incremento
        let incremento = if self.coincide(TokenKind::ParenCerrar) {
            None
        } else {
            let (linea, columna) = (self.linea_actual(), self.columna_actual());
            let nombre = self
                .esperar_identificador("Se esperaba una variable en el incremento del 'para'.")?;
            let valor = if let Some(operador) = self.tomar_operador_compuesto() {
                let derecha = self.parsear_expresion()?;
                Expresion::Binaria {
                    izquierda: Box::new(Expresion::Identificador {
                        nombre: nombre.clone(),
                        linea,
                        columna,
                    }),
                    operador,
                    derecha: Box::new(derecha),
                }
            } else {
                self.esperar(TokenKind::Igual, "Se esperaba '=' en el incremento.")?;
                self.parsear_expresion()?
            };
            Some(Box::new(Declaracion::Asignacion {
                nombre,
                valor: Box::new(valor),
                linea,
                columna,
            }))
        };

        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después del incremento.",
        )?;
        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el bloque del 'para'.",
        )?;
        let bloque = self.parsear_bloque()?;

        Ok(Some(Declaracion::Para {
            inicializacion,
            condicion,
            incremento,
            bloque,
        }))
    }

    /// repetir (<cantidad>) { <bloque> }
    pub(crate) fn parsear_repetir(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'repetir'
        self.esperar(
            TokenKind::ParenAbrir,
            "Se esperaba '(' después de 'repetir'.",
        )?;
        let cantidad = self.parsear_expresion()?;
        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después de la cantidad.",
        )?;
        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el bloque del 'repetir'.",
        )?;
        let bloque = self.parsear_bloque()?;

        Ok(Some(Declaracion::Repetir {
            cantidad: Box::new(cantidad),
            bloque,
        }))
    }

    /// retornar [<expr>]
    pub(crate) fn parsear_retornar(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'retornar'

        let valor = if self.es_inicio_expresion() {
            Some(self.parsear_expresion()?)
        } else {
            None
        };

        Ok(Some(Declaracion::Retornar { valor }))
    }

    /// importar ruta/de/modulo  (sin comillas)  o  importar "ruta/absoluta" (con comillas, obligatorio para absolutas)
    pub(crate) fn parsear_importar(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'importar'

        let ruta = match &self.peek().kind {
            // importar "ruta" — con comillas. Se aceptan tanto rutas absolutas
            // como relativas (alias retrocompatible: `importar "std/io"` es
            // equivalente a `importar std/io`).
            TokenKind::Texto(s) => {
                let ruta = s.clone();
                self.avanzar();
                ruta
            }
            // importar ruta/de/modulo  (sin comillas)
            TokenKind::Identificador(_)
            | TokenKind::ResultadoKw
            | TokenKind::TipoTexto
            | TokenKind::TipoEntero
            | TokenKind::TipoDecimal
            | TokenKind::TipoBooleano
            | TokenKind::TipoExacto => {
                let mut partes: Vec<String> = Vec::new();
                loop {
                    match &self.peek().kind {
                        TokenKind::Identificador(id) => {
                            partes.push(id.clone());
                            self.avanzar();
                        }
                        TokenKind::ResultadoKw => {
                            partes.push("resultado".to_string());
                            self.avanzar();
                        }
                        TokenKind::TipoTexto => {
                            partes.push("Texto".to_string());
                            self.avanzar();
                        }
                        TokenKind::TipoEntero => {
                            partes.push("Entero".to_string());
                            self.avanzar();
                        }
                        TokenKind::TipoDecimal => {
                            partes.push("Decimal".to_string());
                            self.avanzar();
                        }
                        TokenKind::TipoBooleano => {
                            partes.push("Booleano".to_string());
                            self.avanzar();
                        }
                        TokenKind::TipoExacto => {
                            partes.push("Exacto".to_string());
                            self.avanzar();
                        }
                        TokenKind::Dividido => {
                            self.avanzar(); // consume /
                            continue;
                        }
                        _ => break,
                    }
                }
                if partes.is_empty() {
                    return Err(ErrorForja::con_codigo(
                        ErrorTipo::ErrorSintactico,
                        self.linea_actual(),
                        self.columna_actual(),
                        Span::punto(self.columna_actual()),
                        "Se esperaba un nombre de módulo después de 'importar'.",
                        "Ejemplo: importar std/io",
                    ));
                }
                let ruta = partes.join("/");
                if es_ruta_absoluta(&ruta) {
                    return Err(ErrorForja::con_codigo(
                        ErrorTipo::ErrorSintactico,
                        self.linea_actual(),
                        self.columna_actual(),
                        Span::punto(self.columna_actual()),
                        "Las rutas absolutas requieren comillas.",
                        "Usá importar \"ruta/absoluta\" para rutas del sistema de archivos.",
                    ));
                }
                ruta
            }
            _ => {
                return Err(ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    self.columna_actual(),
                    Span::punto(self.columna_actual()),
                    "Se esperaba una ruta después de 'importar'.",
                    "Ejemplos: importar std/io  o  importar \"/ruta/absoluta\"",
                ))
            }
        };
        Ok(Some(Declaracion::Importar(ruta)))
    }

    /// importar externa "ruta.dll"  —  cargar librería compartida (FFI)
    pub(crate) fn parsear_importar_externa(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'importar'
        self.avanzar(); // consume 'externo'/'externa'

        let ruta = match &self.peek().kind {
            TokenKind::Texto(s) => {
                let ruta = s.clone();
                self.avanzar();
                ruta
            }
            _ => {
                return Err(ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    self.columna_actual(),
                    Span::punto(self.columna_actual()),
                    "Se esperaba una ruta entre comillas después de 'importar externa'.",
                    "Ejemplo: importar externa \"liberia.dll\"",
                ))
            }
        };
        Ok(Some(Declaracion::ImportarExterna(ruta)))
    }

    /// hilo { <cuerpo> }  → como declaración
    pub(crate) fn parsear_hilo(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consumir 'hilo'
        self.esperar(TokenKind::LlaveAbrir, "Se esperaba '{' después de 'hilo'.")?;
        let cuerpo = self.parsear_bloque()?;
        // parsear_bloque ya consume '}'
        Ok(Some(Declaracion::Expresion(Expresion::Hilo { cuerpo })))
    }

    /// canal() o canal(capacidad) → como declaración
    pub(crate) fn parsear_canal(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consumir 'canal'
        let capacidad = self.parsear_canal_capacidad()?;
        Ok(Some(Declaracion::Expresion(Expresion::CanalNuevo(
            capacidad,
        ))))
    }

    /// Argumento opcional de `canal(...)`: la capacidad de la cola.
    pub(crate) fn parsear_canal_capacidad(&mut self) -> Result<Option<Box<Expresion>>, ErrorForja> {
        self.esperar(TokenKind::ParenAbrir, "Se esperaba '(' después de 'canal'.")?;
        if self.coincide(TokenKind::ParenCerrar) {
            self.avanzar();
            return Ok(None);
        }
        let capacidad = self.parsear_expresion()?;
        self.esperar(TokenKind::ParenCerrar, "Se esperaba ')' después de '('.")?;
        Ok(Some(Box::new(capacidad)))
    }

    /// seleccionar {
    ///     caso canal -> variable -> { ... }
    ///     tiempo ms { ... }
    ///     otro -> { ... }
    /// }
    pub(crate) fn parsear_seleccionar(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'seleccionar'
        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' después de 'seleccionar'.",
        )?;

        let mut brazos = Vec::new();
        while !self.coincide(TokenKind::LlaveCerrar) && !self.es_eof() {
            if self.coincide(TokenKind::Caso) {
                self.avanzar(); // consume 'caso'
                                // Dos sintaxis posibles:
                                //   caso canal -> variable -> { ... }  (estilo viejo)
                                //   caso variable = expr { ... }        (estilo nuevo, ej: caso valor = rx.recibir())
                                // También: caso tiempo(ms) -> { ... }  (timeout como caso)
                let primero = self.esperar_identificador(
                    "Se esperaba nombre del canal o variable después de 'caso'.",
                )?;

                if self.coincide(TokenKind::Menos) {
                    // Sintaxis vieja: caso canal -> variable -> { ... }
                    self.esperar_flecha()?; // consume ->
                    let var_nombre = self
                        .esperar_identificador("Se esperaba nombre de variable después de '->'.")?;
                    self.esperar_flecha()?; // consume ->
                    self.esperar(
                        TokenKind::LlaveAbrir,
                        "Se esperaba '{' para el cuerpo del caso.",
                    )?;
                    let cuerpo = self.parsear_bloque()?;
                    let (linea_primero, col_primero) = (self.linea_actual(), self.columna_actual());
                    brazos.push(BrazoSeleccionar {
                        recepcion: Some((
                            var_nombre,
                            Expresion::Identificador {
                                nombre: primero,
                                linea: linea_primero,
                                columna: col_primero,
                            },
                        )),
                        timeout_ms: 0,
                        cuerpo,
                    });
                } else if self.coincide(TokenKind::Igual) {
                    // Sintaxis nueva: caso variable = expr { ... }
                    self.avanzar(); // consume '='
                    let expr = self.parsear_expresion()?;
                    self.esperar(
                        TokenKind::LlaveAbrir,
                        "Se esperaba '{' para el cuerpo del caso.",
                    )?;
                    let cuerpo = self.parsear_bloque()?;
                    brazos.push(BrazoSeleccionar {
                        recepcion: Some((primero, expr)),
                        timeout_ms: 0,
                        cuerpo,
                    });
                } else if primero == "tiempo" && self.coincide(TokenKind::ParenAbrir) {
                    // Sintaxis: caso tiempo(ms) -> { ... }  (timeout como caso)
                    self.avanzar(); // consume (
                    let tiempo_expr = self.parsear_expresion()?;
                    self.esperar(
                        TokenKind::ParenCerrar,
                        "Se esperaba ')' después del tiempo.",
                    )?;
                    let timeout_ms = extraer_numero(&tiempo_expr);
                    // '->' opcional
                    if self.coincide(TokenKind::Menos) {
                        self.esperar_flecha()?;
                    }
                    self.esperar(
                        TokenKind::LlaveAbrir,
                        "Se esperaba '{' después del timeout.",
                    )?;
                    let cuerpo = self.parsear_bloque()?;
                    brazos.push(BrazoSeleccionar {
                        recepcion: None,
                        timeout_ms,
                        cuerpo,
                    });
                } else {
                    return Err(ErrorForja::con_codigo(
                        crate::error::ErrorTipo::ErrorSintactico,
                        self.linea_actual(),
                        self.columna_actual(),
                        Span::punto(self.columna_actual()),
                        &format!("Se esperaba '->' o '=' después del nombre en 'caso', pero se encontró: {}", self.peek().kind),
                        "Usá: caso canal -> variable -> { ... } o caso variable = expresion { ... }",
                    ));
                }
            } else if self.coincide(TokenKind::Tiempo) {
                self.avanzar(); // consume 'tiempo'
                                // tiempo milisegundos { ... }
                let tiempo_expr = self.parsear_expresion_primaria()?;
                let timeout_ms = extraer_numero(&tiempo_expr);
                self.esperar(
                    TokenKind::LlaveAbrir,
                    "Se esperaba '{' después del timeout.",
                )?;
                let cuerpo = self.parsear_bloque()?;
                brazos.push(BrazoSeleccionar {
                    recepcion: None,
                    timeout_ms,
                    cuerpo,
                });
            } else if self.coincide(TokenKind::Otro) {
                self.avanzar(); // consume 'otro'
                                // otro -> { ... }  (flecha opcional)
                if self.coincide(TokenKind::Menos) {
                    self.esperar_flecha()?;
                }
                self.esperar(TokenKind::LlaveAbrir, "Se esperaba '{' después de 'otro'.")?;
                let cuerpo = self.parsear_bloque()?;
                brazos.push(BrazoSeleccionar {
                    recepcion: None,
                    timeout_ms: 0,
                    cuerpo,
                });
            } else {
                // Token inesperado dentro de seleccionar
                return Err(ErrorForja::con_codigo(
                    crate::error::ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    self.columna_actual(),
                    Span::punto(self.columna_actual()),
                    &format!("Se esperaba 'caso', 'tiempo' u 'otro' dentro de 'seleccionar', pero se encontró: {}", self.peek().kind),
                    "Usá: seleccionar { caso canal -> variable -> { ... } tiempo 1000 { ... } otro -> { ... } }",
                ));
            }
        }
        self.esperar(
            TokenKind::LlaveCerrar,
            "Se esperaba '}' para cerrar 'seleccionar'.",
        )?;
        Ok(Some(Declaracion::Expresion(Expresion::Seleccionar {
            brazos,
        })))
    }

    /// Consume la flecha -> (Menos seguido de Mayor)
    pub(crate) fn esperar_flecha(&mut self) -> Result<(), ErrorForja> {
        if self.coincide(TokenKind::Menos) {
            let col = self.columna_actual();
            self.avanzar();
            if self.coincide(TokenKind::Mayor) {
                self.avanzar();
                Ok(())
            } else {
                Err(ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    col,
                    Span::rango(col, col + 1),
                    "Se esperaba '->'.",
                    "Usá '->' (guión seguido de mayor que).",
                ))
            }
        } else {
            Err(ErrorForja::con_codigo(
                ErrorTipo::ErrorSintactico,
                self.linea_actual(),
                self.columna_actual(),
                Span::punto(self.columna_actual()),
                "Se esperaba '->'.",
                "Usá '->' (guión seguido de mayor que).",
            ))
        }
    }

    /// Parsea un statement que comienza con una expresión
    pub(crate) fn parsear_expresion_sentencia(
        &mut self,
    ) -> Result<Option<Declaracion>, ErrorForja> {
        // Identificador: nombre o nombre.metodo() o nombre.campo = valor
        if let TokenKind::Identificador(nombre) = &self.peek().kind {
            let nombre = nombre.clone();
            let tok = self.peek().clone();
            let linea = tok.linea;
            let columna = tok.columna;
            self.avanzar();
            return self.parsear_post_identificador(nombre, linea, columna);
        }

        // escribir() function call
        if self.coincide(TokenKind::Escribir) {
            self.avanzar();
            self.esperar(
                TokenKind::ParenAbrir,
                "Se esperaba '(' después de 'escribir'.",
            )?;
            let argumentos = self.parsear_argumentos()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los argumentos.",
            )?;
            return Ok(Some(Declaracion::LlamadaFuncion {
                nombre: "escribir".to_string(),
                argumentos,
            }));
        }

        // este.campo = valor  o  este.metodo()
        if self.coincide(TokenKind::Este) {
            let tok = self.peek().clone();
            let linea = tok.linea;
            let columna = tok.columna;
            self.avanzar();
            return self.parsear_post_identificador("self".to_string(), linea, columna);
        }

        // nuevo como nombre de variable si NO sigue un Identificador (nombre de clase)
        if self.coincide(TokenKind::Nuevo) {
            let es_instanciacion = self.pos + 1 < self.tokens.len()
                && matches!(&self.tokens[self.pos + 1].kind, TokenKind::Identificador(_));
            if !es_instanciacion {
                let tok = self.peek().clone();
                let linea = tok.linea;
                let columna = tok.columna;
                self.avanzar();
                return self.parsear_post_identificador("nuevo".to_string(), linea, columna);
            }
        }

        // Para todo lo demás, parsear como expresión primaria
        let expr = self.parsear_expresion_primaria()?;
        Ok(Some(Declaracion::Expresion(expr)))
    }
}

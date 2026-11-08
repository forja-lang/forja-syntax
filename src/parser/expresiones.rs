use super::*;

impl Parser {
    /// Parsea lo que sigue a un identificador/objeto:
    /// - ident = expr         (asignación)
    /// - ident.miembro = expr (asignación a miembro)
    /// - ident.miembro()      (llamada a método)
    /// - ident.miembro        (acceso a miembro)
    /// - ident(args)          (llamada a función)
    /// - ident                (solo identificador)
    pub(crate) fn parse_post_identificador(
        &mut self,
        nombre: String,
        linea: usize,
        columna: usize,
    ) -> Result<Option<Declaracion>, ErrorForja> {
        // CASO 1: nombre(args) — llamada a función simple (sin encadenamiento)
        // Debe devolverse como Declaracion::LlamadaFuncion (sin Pop, manejado por el compilador)
        if self.coincide(TokenKind::ParenAbrir) {
            self.avanzar();
            let argumentos = self.parse_argumentos()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los argumentos.",
            )?;
            return Ok(Some(Declaracion::LlamadaFuncion { nombre, argumentos }));
        }

        // CASO 2: nombre = expr  (asignación simple)
        if self.coincide(TokenKind::Igual) {
            self.avanzar();
            let valor = self.parse_expresion()?;
            return Ok(Some(Declaracion::Asignacion {
                nombre,
                valor: Box::new(valor),
                linea,
                columna,
            }));
        }

        // CASO 2b: nombre += expr  (asignación compuesta)
        if let Some(operador) = self.tomar_operador_compuesto() {
            let derecha = self.parse_expresion()?;
            return Ok(Some(Declaracion::Asignacion {
                nombre: nombre.clone(),
                valor: Box::new(Expresion::Binaria {
                    izquierda: Box::new(Expresion::Identificador {
                        nombre,
                        linea,
                        columna,
                    }),
                    operador,
                    derecha: Box::new(derecha),
                }),
                linea,
                columna,
            }));
        }

        // CASO 3: nombre[índice] o nombre[índice] = expr (acceso/asignación por índice)
        if self.coincide(TokenKind::CorcheteAbrir) {
            self.avanzar(); // consume [
            let indice = self.parse_expresion()?;
            self.esperar(
                TokenKind::CorcheteCerrar,
                "Se esperaba ']' después del índice.",
            )?;

            // nombre[índice] = valor → AsignacionIndex (guarda el array modificado)
            if self.coincide(TokenKind::Igual) {
                self.avanzar();
                let valor = self.parse_expresion()?;
                return Ok(Some(Declaracion::AsignacionIndex {
                    nombre: nombre.clone(),
                    indice: Box::new(indice),
                    valor: Box::new(valor),
                    linea,
                    columna,
                }));
            }

            // nombre[índice] += valor
            if let Some(operador) = self.tomar_operador_compuesto() {
                let valor = self.parse_expresion()?;
                return Ok(Some(Declaracion::Expresion(
                    Expresion::AsignacionCompuesta {
                        objetivo: Box::new(Expresion::Index {
                            objeto: Box::new(Expresion::Identificador {
                                nombre,
                                linea,
                                columna,
                            }),
                            indice: Box::new(indice),
                        }),
                        operador,
                        valor: Box::new(valor),
                    },
                )));
            }

            // nombre[índice][índice2]... o cadenas con . y ()
            // Construimos la expresión base de índice
            let mut expr: Expresion = Expresion::Index {
                objeto: Box::new(Expresion::Identificador {
                    nombre: nombre.clone(),
                    linea,
                    columna,
                }),
                indice: Box::new(indice),
            };
            let mut nombre_compuesto = nombre;

            // Bucle para [índice][índice]... y .miembro etc.
            loop {
                // nombre[índice][índice]...
                if self.coincide(TokenKind::CorcheteAbrir) {
                    self.avanzar();
                    let indice2 = self.parse_expresion()?;
                    self.esperar(
                        TokenKind::CorcheteCerrar,
                        "Se esperaba ']' después del índice.",
                    )?;
                    expr = Expresion::Index {
                        objeto: Box::new(expr),
                        indice: Box::new(indice2),
                    };
                    // `m[i][j] = valor`. Los arreglos son referencias,
                    // así que se escribe en el arreglo interno: `ArraySet` sobre
                    // el `Index` exterior. Antes la asignación se descartaba en
                    // silencio (las matrices 2D de los ejemplos no se llenaban).
                    if self.coincide(TokenKind::Igual) {
                        self.avanzar();
                        let valor = self.parse_expresion()?;
                        return Ok(Some(Declaracion::Expresion(Expresion::ArraySet {
                            array: Box::new(expr),
                            valor: Box::new(valor),
                        })));
                    }
                    // m[i][j] += valor (el objetivo encadenado se evalúa una vez)
                    if let Some(operador) = self.tomar_operador_compuesto() {
                        let valor = self.parse_expresion()?;
                        return Ok(Some(Declaracion::Expresion(
                            Expresion::AsignacionCompuesta {
                                objetivo: Box::new(expr),
                                operador,
                                valor: Box::new(valor),
                            },
                        )));
                    }
                    continue;
                }

                // nombre[índice].miembro o nombre[índice].metodo()
                if self.coincide(TokenKind::Punto) {
                    self.avanzar();
                    let miembro = self.esperar_identificador(
                        "Se esperaba un nombre de miembro después de '.'.",
                    )?;
                    nombre_compuesto = format!("{}.{}", nombre_compuesto, miembro);
                    if self.coincide(TokenKind::ParenAbrir) {
                        self.avanzar();
                        let argumentos = self.parse_argumentos()?;
                        self.esperar(
                            TokenKind::ParenCerrar,
                            "Se esperaba ')' después de los argumentos.",
                        )?;
                        // `a[i].metodo(args)`: se conserva la estructura (el
                        // receptor es una expresión, no un nombre simple). Con
                        // el nombre punteado el IR no podía resolver el objeto y
                        // la llamada quedaba como `Call("metodo")` → "función no
                        // definida".
                        expr = Expresion::LlamadaMetodo {
                            objeto: Box::new(expr),
                            metodo: miembro,
                            argumentos,
                        };
                        continue;
                    }
                    expr = Expresion::AccesoMiembro {
                        objeto: Box::new(expr),
                        miembro,
                    };
                    continue;
                }

                break;
            }

            // nombre[índice] como expresión de lectura
            return Ok(Some(Declaracion::Expresion(expr)));
        }

        // CASO 4: nombre.miembro, nombre.miembro(), nombre.miembro[índice] = valor (acceso a miembro)
        if self.coincide(TokenKind::Punto) {
            self.avanzar();
            let miembro =
                self.esperar_identificador("Se esperaba un nombre de miembro después de '.'.")?;
            let nombre_clon = nombre.clone();
            let mut nombre_compuesto = format!("{}.{}", nombre, miembro);

            // nombre.miembro = valor (asignación a campo)
            if self.coincide(TokenKind::Igual) {
                self.avanzar();
                let valor = self.parse_expresion()?;
                return Ok(Some(Declaracion::Expresion(Expresion::AsignacionCampo {
                    objeto: Box::new(Expresion::Identificador {
                        nombre: nombre_clon,
                        linea,
                        columna,
                    }),
                    campo: miembro,
                    valor: Box::new(valor),
                })));
            }

            // nombre.miembro += valor (asignación compuesta a campo)
            if let Some(operador) = self.tomar_operador_compuesto() {
                let valor = self.parse_expresion()?;
                return Ok(Some(Declaracion::Expresion(
                    Expresion::AsignacionCompuesta {
                        objetivo: Box::new(Expresion::AccesoMiembro {
                            objeto: Box::new(Expresion::Identificador {
                                nombre: nombre_clon,
                                linea,
                                columna,
                            }),
                            miembro,
                        }),
                        operador,
                        valor: Box::new(valor),
                    },
                )));
            }

            // nombre.miembro(args) - llamada a método
            if self.coincide(TokenKind::ParenAbrir) {
                self.avanzar();
                let argumentos = self.parse_argumentos()?;
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' después de los argumentos.",
                )?;
                // Devolver como Declaracion::LlamadaFuncion (sin Pop extra)
                return Ok(Some(Declaracion::LlamadaFuncion {
                    nombre: format!("{}.{}", nombre, miembro),
                    argumentos,
                }));
            }

            // nombre.miembro[índice] o nombre.miembro[índice] = valor
            if self.coincide(TokenKind::CorcheteAbrir) {
                self.avanzar();
                let indice = self.parse_expresion()?;
                self.esperar(
                    TokenKind::CorcheteCerrar,
                    "Se esperaba ']' después del índice.",
                )?;

                // nombre.miembro[índice] += valor
                if let Some(operador) = self.tomar_operador_compuesto() {
                    let valor = self.parse_expresion()?;
                    return Ok(Some(Declaracion::Expresion(
                        Expresion::AsignacionCompuesta {
                            objetivo: Box::new(Expresion::Index {
                                objeto: Box::new(Expresion::AccesoMiembro {
                                    objeto: Box::new(Expresion::Identificador {
                                        nombre: nombre.clone(),
                                        linea,
                                        columna,
                                    }),
                                    miembro: miembro.clone(),
                                }),
                                indice: Box::new(indice),
                            }),
                            operador,
                            valor: Box::new(valor),
                        },
                    )));
                }

                // nombre.miembro[índice] = valor
                if self.coincide(TokenKind::Igual) {
                    self.avanzar();
                    let valor = self.parse_expresion()?;
                    return Ok(Some(Declaracion::Expresion(Expresion::ArraySet {
                        array: Box::new(Expresion::Index {
                            objeto: Box::new(Expresion::AccesoMiembro {
                                objeto: Box::new(Expresion::Identificador {
                                    nombre,
                                    linea,
                                    columna,
                                }),
                                miembro,
                            }),
                            indice: Box::new(indice),
                        }),
                        valor: Box::new(valor),
                    })));
                }

                // nombre.miembro[índice] - acceso de lectura
                let mut expr = Expresion::AccesoMiembro {
                    objeto: Box::new(Expresion::Identificador {
                        nombre: nombre_clon,
                        linea,
                        columna,
                    }),
                    miembro,
                };

                // Bucle para encadenar nombre.miembro[índice][índice2]...
                loop {
                    if self.coincide(TokenKind::CorcheteAbrir) {
                        self.avanzar();
                        let indice2 = self.parse_expresion()?;
                        self.esperar(
                            TokenKind::CorcheteCerrar,
                            "Se esperaba ']' después del índice.",
                        )?;
                        if self.coincide(TokenKind::Igual) {
                            self.avanzar();
                            let valor = self.parse_expresion()?;
                            // Se aplican **los dos** índices: antes se perdía el
                            // primero y `m[0][1] = 4` escribía en `m[1]`.
                            let full_index = Expresion::Index {
                                objeto: Box::new(Expresion::Index {
                                    objeto: Box::new(expr),
                                    indice: Box::new(indice),
                                }),
                                indice: Box::new(indice2),
                            };
                            return Ok(Some(Declaracion::Expresion(Expresion::ArraySet {
                                array: Box::new(full_index),
                                valor: Box::new(valor),
                            })));
                        }
                        // nombre.miembro[i][j] += valor
                        if let Some(operador) = self.tomar_operador_compuesto() {
                            let valor = self.parse_expresion()?;
                            let full_index = Expresion::Index {
                                objeto: Box::new(Expresion::Index {
                                    objeto: Box::new(expr),
                                    indice: Box::new(indice),
                                }),
                                indice: Box::new(indice2),
                            };
                            return Ok(Some(Declaracion::Expresion(
                                Expresion::AsignacionCompuesta {
                                    objetivo: Box::new(full_index),
                                    operador,
                                    valor: Box::new(valor),
                                },
                            )));
                        }
                        expr = Expresion::Index {
                            objeto: Box::new(expr),
                            indice: Box::new(indice2),
                        };
                        continue;
                    }
                    break;
                }

                // `x.campo[i].metodo(args)`: se sigue encadenando sobre el
                // `Index` en vez de devolverlo como sentencia (antes el
                // `.metodo(...)` posterior se perdía y la llamada no existía).
                let mut base = Expresion::Index {
                    objeto: Box::new(expr),
                    indice: Box::new(indice),
                };
                while self.coincide(TokenKind::Punto) {
                    self.avanzar();
                    let m2 = self.esperar_identificador(
                        "Se esperaba un nombre de miembro después de '.'.",
                    )?;
                    if self.coincide(TokenKind::ParenAbrir) {
                        self.avanzar();
                        let argumentos = self.parse_argumentos()?;
                        self.esperar(
                            TokenKind::ParenCerrar,
                            "Se esperaba ')' después de los argumentos.",
                        )?;
                        base = Expresion::LlamadaMetodo {
                            objeto: Box::new(base),
                            metodo: m2,
                            argumentos,
                        };
                        continue;
                    }
                    base = Expresion::AccesoMiembro {
                        objeto: Box::new(base),
                        miembro: m2,
                    };
                }
                return Ok(Some(Declaracion::Expresion(base)));
            }

            // nombre.miembro o nombre.miembro.submiembro...
            let mut expr = Expresion::AccesoMiembro {
                objeto: Box::new(Expresion::Identificador {
                    nombre: nombre_clon,
                    linea,
                    columna,
                }),
                miembro,
            };
            // Encadenar más .miembro
            loop {
                if self.coincide(TokenKind::Punto) {
                    self.avanzar();
                    let m2 = self.esperar_identificador(
                        "Se esperaba un nombre de miembro después de '.'.",
                    )?;
                    nombre_compuesto = format!("{}.{}", nombre_compuesto, m2);
                    if self.coincide(TokenKind::ParenAbrir) {
                        self.avanzar();
                        let argumentos = self.parse_argumentos()?;
                        self.esperar(
                            TokenKind::ParenCerrar,
                            "Se esperaba ')' después de los argumentos.",
                        )?;
                        return Ok(Some(Declaracion::LlamadaFuncion {
                            nombre: nombre_compuesto,
                            argumentos,
                        }));
                    }
                    expr = Expresion::AccesoMiembro {
                        objeto: Box::new(expr),
                        miembro: m2,
                    };
                    continue;
                }
                break;
            }
            return Ok(Some(Declaracion::Expresion(expr)));
        }

        // CASO 5: Solo identificador
        Ok(Some(Declaracion::Expresion(Expresion::Identificador {
            nombre,
            linea,
            columna,
        })))
    }

    // ============================================================
    // Parsing de expresiones (con precedencia)
    // ============================================================

    pub(crate) fn parse_expresion(&mut self) -> Result<Expresion, ErrorForja> {
        self.parse_expresion_asignacion()
    }

    /// Consume un operador de asignación compuesta y devuelve el operador
    /// aritmético equivalente (`+=` → `+`, etc.). Si no hay, no consume nada.
    pub(crate) fn tomar_operador_compuesto(&mut self) -> Option<Operador> {
        let operador = match self.peek().kind {
            TokenKind::MasIgual => Operador::Suma,
            TokenKind::MenosIgual => Operador::Resta,
            TokenKind::PorIgual => Operador::Multiplicacion,
            TokenKind::DivididoIgual => Operador::Division,
            TokenKind::PorcentajeIgual => Operador::Modulo,
            _ => return None,
        };
        self.avanzar();
        Some(operador)
    }

    /// Asignación como expresión (precedencia más baja): x = 5, a = b = 3
    /// La asignación es asociativa a la derecha. También cubre `x += 1` y sus
    /// variantes (nodo `AsignacionCompuesta`, para evaluar el objetivo una vez).
    pub(crate) fn parse_expresion_asignacion(&mut self) -> Result<Expresion, ErrorForja> {
        let expr = self.parse_expresion_ternario()?;
        let compuesto = self.tomar_operador_compuesto();
        if compuesto.is_none() && !self.coincide(TokenKind::Igual) {
            return Ok(expr);
        }
        let valor = if let Some(operador) = compuesto {
            // El objetivo va tal cual: cada backend lo evalúa una sola vez.
            let valor = self.parse_expresion_asignacion()?;
            match expr {
                Expresion::Identificador { .. }
                | Expresion::AccesoMiembro { .. }
                | Expresion::Index { .. } => {
                    return Ok(Expresion::AsignacionCompuesta {
                        objetivo: Box::new(expr),
                        operador,
                        valor: Box::new(valor),
                    });
                }
                _ => {
                    return Err(ErrorForja::con_codigo(
                        ErrorTipo::ErrorSintactico,
                        self.linea_actual(),
                        self.columna_actual(),
                        Span::punto(self.columna_actual()),
                        &format!(
                            "Expresión inesperada en asignación compuesta: {} ...",
                            self.peek().kind
                        ),
                        "Solo se puede asignar a una variable, campo o índice (arr[i]).",
                    ));
                }
            }
        } else {
            self.avanzar(); // consumir =
            self.parse_expresion_asignacion()? // asociativo a la derecha
        };
        match expr {
            Expresion::Identificador { nombre, .. } => Ok(Expresion::Asignacion {
                variable: nombre,
                valor: Box::new(valor),
            }),
            Expresion::AccesoMiembro { objeto, miembro } => Ok(Expresion::AsignacionCampo {
                objeto,
                campo: miembro,
                valor: Box::new(valor),
            }),
            Expresion::Index { .. } => Ok(Expresion::ArraySet {
                array: Box::new(expr),
                valor: Box::new(valor),
            }),
            _ => Err(ErrorForja::con_codigo(
                ErrorTipo::ErrorSintactico,
                self.linea_actual(),
                self.columna_actual(),
                Span::punto(self.columna_actual()),
                &format!("Expresión inesperada: {} = ...", self.peek().kind),
                "Solo se puede asignar a una variable, campo o índice (arr[i]).",
            )),
        }
    }

    /// Expresiones ternarias: cond ? verdadero : falso
    pub(crate) fn parse_expresion_ternario(&mut self) -> Result<Expresion, ErrorForja> {
        let cond = self.parse_expresion_logica()?;
        if self.coincide(TokenKind::Interrogacion) {
            self.avanzar(); // consumir ?
            let si_verdadero = self.parse_expresion()?;
            self.esperar(
                TokenKind::DosPuntos,
                "Se esperaba ':' en la expresión ternaria (condición ? verdadero : falso).",
            )?;
            let si_falso = self.parse_expresion_ternario()?;
            Ok(Expresion::Ternario {
                condicion: Box::new(cond),
                si_verdadero: Box::new(si_verdadero),
                si_falso: Box::new(si_falso),
            })
        } else {
            Ok(cond)
        }
    }

    /// `y`/`o` son operadores **contextuales**: el lexer los emite como
    /// identificadores para que sigan sirviendo como nombre de variable
    /// (`variable y = 2`). Solo cuentan como operador si están en la misma
    /// línea que el operando anterior: así `x = 5` seguido de una línea
    /// `y = 10` no se confunde con un `y` lógico.
    pub(crate) fn coincide_operador_palabra(&self, palabra: &str) -> bool {
        matches!(&self.peek().kind, TokenKind::Identificador(n) if n == palabra)
            && self.pos > 0
            && self.tokens[self.pos - 1].linea == self.peek().linea
    }

    /// Expresiones lógicas: || (O)
    pub(crate) fn parse_expresion_logica(&mut self) -> Result<Expresion, ErrorForja> {
        let mut expr = self.parse_expresion_y()?;
        while self.coincide(TokenKind::O) || self.coincide_operador_palabra("o") {
            let operador = Operador::O;
            self.avanzar();
            let derecha = self.parse_expresion_y()?;
            expr = Expresion::Binaria {
                izquierda: Box::new(expr),
                operador,
                derecha: Box::new(derecha),
            };
        }
        Ok(expr)
    }

    /// Expresiones lógicas: && (Y)
    pub(crate) fn parse_expresion_y(&mut self) -> Result<Expresion, ErrorForja> {
        let mut expr = self.parse_expresion_igualdad()?;
        while self.coincide(TokenKind::Y) || self.coincide_operador_palabra("y") {
            let operador = Operador::Y;
            self.avanzar();
            let derecha = self.parse_expresion_igualdad()?;
            expr = Expresion::Binaria {
                izquierda: Box::new(expr),
                operador,
                derecha: Box::new(derecha),
            };
        }
        Ok(expr)
    }

    /// Expresiones de igualdad: ==, !=
    pub(crate) fn parse_expresion_igualdad(&mut self) -> Result<Expresion, ErrorForja> {
        let mut expr = self.parse_expresion_relacional()?;
        while self.coincide(TokenKind::IgualIgual) || self.coincide(TokenKind::Diferente) {
            let operador = if self.coincide(TokenKind::IgualIgual) {
                self.avanzar();
                Operador::IgualIgual
            } else {
                self.avanzar();
                Operador::Diferente
            };
            let derecha = self.parse_expresion_relacional()?;
            expr = Expresion::Binaria {
                izquierda: Box::new(expr),
                operador,
                derecha: Box::new(derecha),
            };
        }
        Ok(expr)
    }

    /// Expresiones relacionales: >, <, >=, <=
    pub(crate) fn parse_expresion_relacional(&mut self) -> Result<Expresion, ErrorForja> {
        let mut expr = self.parse_expresion_aditiva()?;
        while self.coincide(TokenKind::Mayor)
            || self.coincide(TokenKind::Menor)
            || self.coincide(TokenKind::MayorIgual)
            || self.coincide(TokenKind::MenorIgual)
        {
            let operador = match self.peek().kind {
                TokenKind::Mayor => {
                    self.avanzar();
                    Operador::Mayor
                }
                TokenKind::Menor => {
                    self.avanzar();
                    Operador::Menor
                }
                TokenKind::MayorIgual => {
                    self.avanzar();
                    Operador::MayorIgual
                }
                TokenKind::MenorIgual => {
                    self.avanzar();
                    Operador::MenorIgual
                }
                _ => unreachable!(),
            };
            let derecha = self.parse_expresion_aditiva()?;
            expr = Expresion::Binaria {
                izquierda: Box::new(expr),
                operador,
                derecha: Box::new(derecha),
            };
        }
        Ok(expr)
    }

    /// Expresiones aditivas: +, -
    pub(crate) fn parse_expresion_aditiva(&mut self) -> Result<Expresion, ErrorForja> {
        let mut expr = self.parse_expresion_multiplicativa()?;
        while self.coincide(TokenKind::Mas) || self.coincide(TokenKind::Menos) {
            let operador = if self.coincide(TokenKind::Mas) {
                self.avanzar();
                Operador::Suma
            } else {
                self.avanzar();
                Operador::Resta
            };
            let derecha = self.parse_expresion_multiplicativa()?;
            expr = Expresion::Binaria {
                izquierda: Box::new(expr),
                operador,
                derecha: Box::new(derecha),
            };
        }
        Ok(expr)
    }

    /// Expresiones multiplicativas: *, /, %
    pub(crate) fn parse_expresion_multiplicativa(&mut self) -> Result<Expresion, ErrorForja> {
        let mut expr = self.parse_expresion_unaria()?;
        while self.coincide(TokenKind::Por)
            || self.coincide(TokenKind::Dividido)
            || self.coincide(TokenKind::Porcentaje)
        {
            let operador = if self.coincide(TokenKind::Por) {
                self.avanzar();
                Operador::Multiplicacion
            } else if self.coincide(TokenKind::Porcentaje) {
                self.avanzar();
                Operador::Modulo
            } else {
                self.avanzar();
                Operador::Division
            };
            let derecha = self.parse_expresion_unaria()?;
            expr = Expresion::Binaria {
                izquierda: Box::new(expr),
                operador,
                derecha: Box::new(derecha),
            };
        }
        Ok(expr)
    }

    /// Expresiones unarias: !expr, -expr, &expr
    pub(crate) fn parse_expresion_unaria(&mut self) -> Result<Expresion, ErrorForja> {
        if self.coincide(TokenKind::No) {
            self.avanzar();
            let expr = self.parse_expresion_unaria()?;
            return Ok(Expresion::Unaria {
                operador: OperadorUnario::No,
                expr: Box::new(expr),
            });
        }

        if self.coincide(TokenKind::Menos) {
            // Podría ser un número negativo o resta unaria
            let _col = self.columna_actual();
            self.avanzar();
            // Si sigue un número, crear literal negativo
            if self.coincide(TokenKind::Numero(i64::MIN)) {
                if let TokenKind::Numero(n) = self.peek().kind {
                    self.avanzar();
                    return Ok(Expresion::LiteralNumero(-n));
                }
            }
            let expr = self.parse_expresion_unaria()?;
            return Ok(Expresion::Unaria {
                operador: OperadorUnario::Negar,
                expr: Box::new(expr),
            });
        }

        if self.coincide(TokenKind::Amp) {
            self.avanzar();
            let mutable = self.coincide(TokenKind::Mut);
            if mutable {
                self.avanzar();
            }
            let expr = self.parse_expresion_unaria()?;
            return Ok(Expresion::Referencia {
                expr: Box::new(expr),
                mutable,
            });
        }

        self.parse_expresion_primaria()
    }

    /// Expresiones primarias: literales, identificadores, llamadas, etc.
    pub(crate) fn parse_expresion_primaria(&mut self) -> Result<Expresion, ErrorForja> {
        let expr = self.parse_expresion_primaria_interna()?;
        // Después de parsear una primaria, verificar si hay postfijo (.method() o ())
        self.parse_postfijo(expr)
    }
}

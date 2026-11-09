use super::*;

impl Parser {
    /// Parsea un tipo (para anotaciones de tipo)
    pub(crate) fn parsear_tipo(&mut self) -> Result<Tipo, ErrorForja> {
        self.parsear_tipo_interno()
    }

    /// Implementación interna de parsear_tipo (profundidad ya verificada)
    pub(crate) fn parsear_tipo_interno(&mut self) -> Result<Tipo, ErrorForja> {
        let token = self.peek().clone();
        match token.kind {
            TokenKind::TipoEntero => {
                self.avanzar();
                Ok(Tipo::Entero)
            }
            TokenKind::TipoDecimal => {
                self.avanzar();
                Ok(Tipo::Decimal)
            }
            TokenKind::TipoTexto => {
                self.avanzar();
                Ok(Tipo::Texto)
            }
            TokenKind::TipoBooleano => {
                self.avanzar();
                Ok(Tipo::Booleano)
            }
            TokenKind::TipoExacto => {
                self.avanzar();
                Ok(Tipo::Exacto)
            }
            TokenKind::Identificador(s) => {
                let nombre = s.clone();
                self.avanzar();

                // Genérico sólo si fue declarado en un `<...>` en ámbito. Nunca por
                // la forma del nombre: `tipo F =...` es un tipo real.
                let es_parametro_tipo = self.genericos.iter().any(|g| g == &nombre);

                // Verificar si sigue < para tipos genéricos: Nombre<T> o Nombre<T, E>
                if self.coincide(TokenKind::Menor) {
                    self.avanzar(); // consumir <
                    let tipo_params = self.parsear_lista_tipos()?;
                    self.esperar(
                        TokenKind::Mayor,
                        "Se esperaba '>' para cerrar el tipo genérico.",
                    )?;

                    match nombre.as_str() {
                        "Resultado" if tipo_params.len() == 2 => Ok(Tipo::Resultado(
                            Box::new(tipo_params[0].clone()),
                            Box::new(tipo_params[1].clone()),
                        )),
                        "Opcion" if tipo_params.len() == 1 => {
                            Ok(Tipo::Opcion(Box::new(tipo_params[0].clone())))
                        }
                        _ => {
                            // Para tipos genéricos definidos por usuario (Caja<Entero>, Par<T,U>, etc.)
                            // Simplificado: ignoramos los parámetros internos, solo almacenamos el nombre
                            Ok(Tipo::Clase(nombre))
                        }
                    }
                } else if es_parametro_tipo {
                    // Es un parámetro de tipo genérico (T, U, V, etc.)
                    Ok(Tipo::Parametro(nombre))
                } else {
                    let tipo = match nombre.as_str() {
                        "Entero" => Tipo::Entero,
                        "Decimal" => Tipo::Decimal,
                        "Texto" => Tipo::Texto,
                        "Booleano" => Tipo::Booleano,
                        "Exacto" => Tipo::Exacto,
                        _ => Tipo::Clase(nombre),
                    };
                    Ok(tipo)
                }
            }
            TokenKind::ParenAbrir => {
                // Tipo tupla: (Tipo1, Tipo2, ...) → consumir y retornar Clase("auto")
                self.avanzar(); // consumir (
                while !self.coincide(TokenKind::ParenCerrar) && !self.es_eof() {
                    self.parsear_tipo()?; // consumir cada tipo
                    if self.coincide(TokenKind::Coma) {
                        self.avanzar();
                    }
                }
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' para cerrar el tipo tupla.",
                )?;
                Ok(Tipo::Clase("auto".to_string()))
            }
            _ => {
                self.avanzar();
                Ok(Tipo::Clase("auto".to_string()))
            }
        }
    }

    /// Parsea una lista de tipos separados por coma (para genéricos: <T, E>)
    pub(crate) fn parsear_lista_tipos(&mut self) -> Result<Vec<Tipo>, ErrorForja> {
        let mut tipos = Vec::new();
        tipos.push(self.parsear_tipo()?);
        while self.coincide(TokenKind::Coma) {
            self.avanzar();
            if self.coincide(TokenKind::Mayor) {
                break;
            }
            tipos.push(self.parsear_tipo()?);
        }
        Ok(tipos)
    }

    // ============================================================
    // Métodos auxiliares
    // ============================================================

    /// Parsea un bloque entre llaves
    pub(crate) fn parsear_bloque(&mut self) -> Result<Vec<Declaracion>, ErrorForja> {
        let mut declaraciones = Vec::new();

        while !self.coincide(TokenKind::LlaveCerrar) && !self.es_eof() {
            match self.parsear_declaracion() {
                Ok(Some(decl)) => declaraciones.push(decl),
                Ok(None) => break,
                Err(mut err) => {
                    if err.span_bytes.is_none() {
                        if let Some(span) = self.peek().span {
                            err = err.con_span_bytes(span);
                        }
                    }
                    self.errores.push(err);
                    self.sincronizar();
                }
            }
        }

        // Consume el } si existe
        if self.coincide(TokenKind::LlaveCerrar) {
            self.avanzar();
        } else {
            return Err(ErrorForja::con_codigo(
                ErrorTipo::ErrorSintactico,
                self.linea_actual(),
                self.columna_actual(),
                Span::punto(self.columna_actual()),
                "Se esperaba '}' para cerrar el bloque.",
                "Agregá '}' al final del bloque para cerrar la llave.",
            ));
        }

        Ok(declaraciones)
    }

    /// Recuperación de errores: avanza hasta un punto seguro (;, }, o keyword)
    /// V-10: Limitado a 100 tokens para evitar bucles infinitos
    pub(crate) fn sincronizar(&mut self) {
        self.avanzar();
        let mut tokens_skipped = 0;
        const MAX_SKIP: usize = 100;
        while !self.es_eof() && tokens_skipped < MAX_SKIP {
            if self.coincide(TokenKind::PuntoComa)
                || self.coincide(TokenKind::LlaveCerrar)
                || self.coincide(TokenKind::Variable)
                || self.coincide(TokenKind::Funcion)
                || self.coincide(TokenKind::Clase)
                || self.coincide(TokenKind::Si)
                || self.coincide(TokenKind::Mientras)
                || self.coincide(TokenKind::Para)
                || self.coincide(TokenKind::Repetir)
                || self.coincide(TokenKind::Coincidir)
                || self.coincide(TokenKind::Caso)
            {
                return;
            }
            self.avanzar();
            tokens_skipped += 1;
        }
    }

    /// Recolecta todos los doc comments (///) consecutivos y los concatena
    pub(crate) fn recolectar_doc_comments(&mut self) -> Option<String> {
        let mut partes = Vec::new();
        while self.coincide(TokenKind::DocComment(String::new())) {
            if let TokenKind::DocComment(ref doc) = self.peek().kind {
                partes.push(doc.clone());
                self.avanzar();
            } else {
                break;
            }
        }
        if partes.is_empty() {
            None
        } else {
            Some(partes.join("\n"))
        }
    }

    /// Verifica si el token actual es de cierto tipo
    pub(crate) fn coincide(&self, kind: TokenKind) -> bool {
        if self.pos >= self.tokens.len() {
            return false;
        }
        // Para tipos con datos, comparamos variante (no el valor exacto)
        let actual = &self.tokens[self.pos].kind;
        std::mem::discriminant(actual) == std::mem::discriminant(&kind)
    }

    /// Obtiene el token actual
    pub(crate) fn peek(&self) -> &Token {
        if self.pos >= self.tokens.len() {
            &self.tokens[self.tokens.len() - 1] // EOF
        } else {
            &self.tokens[self.pos]
        }
    }
}

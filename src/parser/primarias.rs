use super::*;

impl Parser {
    /// Parsea el núcleo de una expresión primaria (sin postfijo)
    pub(crate) fn parse_expresion_primaria_interna(&mut self) -> Result<Expresion, ErrorForja> {
        if self.coincide(TokenKind::LiteralExacto(0, 0))
            || self.coincide(TokenKind::LiteralExacto(i128::MIN, 0))
        {
            if let TokenKind::LiteralExacto(coeff, scale) = self.peek().kind {
                self.avanzar();
                return Ok(Expresion::LiteralExacto(coeff, scale));
            }
        }

        if self.coincide(TokenKind::Numero(0)) || self.coincide(TokenKind::Numero(i64::MIN)) {
            if let TokenKind::Numero(n) = self.peek().kind {
                self.avanzar();
                return Ok(Expresion::LiteralNumero(n));
            }
        }

        if self.coincide(TokenKind::Decimal {
            valor: 0.0,
            coeff: 0,
            escala: 0,
        }) {
            if let TokenKind::Decimal { valor, .. } = self.peek().kind.clone() {
                self.avanzar();
                return Ok(Expresion::LiteralDecimal(valor));
            }
        }

        if self.coincide(TokenKind::Texto(String::new()))
            || self.coincide(TokenKind::Texto("".to_string()))
        {
            if let TokenKind::Texto(ref s) = self.peek().kind {
                let s = s.clone();
                let linea_texto = self.peek().linea;
                self.avanzar();
                // Verificar si este Texto es parte de una interpolación
                // Solo si el siguiente token está en la MISMA línea (si está en otra línea,
                // es una declaración separada, no interpolación).
                if self.hay_interpolacion() && linea_texto == self.peek().linea {
                    return self.parse_string_interpolado(s, linea_texto);
                }
                return Ok(Expresion::LiteralTexto(s));
            }
        }

        // Caracter literal: 'A' → valor ASCII como i64
        if self.coincide(TokenKind::Caracter(' ')) {
            if let TokenKind::Caracter(c) = self.peek().kind.clone() {
                self.avanzar();
                return Ok(Expresion::LiteralNumero(c as i64));
            }
        }

        if self.coincide(TokenKind::Verdadero) {
            self.avanzar();
            return Ok(Expresion::LiteralBooleano(true));
        }

        if self.coincide(TokenKind::Falso) {
            self.avanzar();
            return Ok(Expresion::LiteralBooleano(false));
        }

        if self.coincide(TokenKind::Nulo) {
            self.avanzar();
            return Ok(Expresion::LiteralNulo);
        }

        // Closure: func(param) { cuerpo } — función anónima como valor
        if self.coincide(TokenKind::Funcion) {
            self.avanzar();
            self.esperar(
                TokenKind::ParenAbrir,
                "Se esperaba '(' después de 'func' en el closure.",
            )?;
            let parametros = self.parse_parametros()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los parámetros del closure.",
            )?;
            self.esperar(
                TokenKind::LlaveAbrir,
                "Se esperaba '{' para el cuerpo del closure.",
            )?;
            let cuerpo = self.parse_bloque()?;
            return Ok(Expresion::Closure { parametros, cuerpo });
        }

        if self.coincide(TokenKind::ParenAbrir) {
            self.profundidad_paren += 1;
            if self.profundidad_paren > MAX_ANIDAMIENTO_PAREN {
                self.profundidad_paren -= 1;
                return Err(ErrorForja::con_codigo(
                    ErrorTipo::DemasiadaAnidacion { max: MAX_ANIDAMIENTO_PAREN as u32 },
                    self.peek().linea,
                    self.peek().columna,
                    Span::punto(self.peek().columna),
                    "Demasiados paréntesis anidados",
                    &format!("El parser permite máximo {} niveles de anidamiento de paréntesis para evitar stack overflow. Refactoriza la expresión.",
                        MAX_ANIDAMIENTO_PAREN),
                ));
            }
            self.avanzar();
            let first = self.parse_expresion()?;
            self.profundidad_paren -= 1;
            // Si después de la primera expresión hay coma, es una tupla (a, b, ...)
            if self.coincide(TokenKind::Coma) {
                let mut elementos = vec![first];
                while self.coincide(TokenKind::Coma) {
                    self.avanzar();
                    if self.coincide(TokenKind::ParenCerrar) {
                        break;
                    }
                    elementos.push(self.parse_expresion()?);
                }
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' para cerrar la tupla.",
                )?;
                return Ok(Expresion::Arreglo(elementos));
            }
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' para cerrar la expresión.",
            )?;
            return Ok(Expresion::Grupo(Box::new(first)));
        }

        if self.coincide(TokenKind::CorcheteAbrir) {
            return self.parse_arreglo();
        }

        // Mapa literal: {"clave": valor, ...}
        if self.coincide(TokenKind::LlaveAbrir) {
            return self.parse_mapa();
        }

        // Identificador, llamada a función, acceso a miembro o instanciación
        if self.coincide(TokenKind::Identificador(String::new()))
            || self.coincide(TokenKind::Identificador("".to_string()))
        {
            let nombre = if let TokenKind::Identificador(ref id) = self.peek().kind {
                id.clone()
            } else {
                unreachable!()
            };
            let (id_linea, id_columna) = (self.peek().linea, self.peek().columna);
            self.avanzar();

            // Detectar Ok(), Error(), Algo(), Alguno(), Some(), Nada(), Ninguno() como constructores especiales
            if (nombre == "Ok"
                || nombre == "Error"
                || nombre == "Algo"
                || nombre == "Alguno"
                || nombre == "Some"
                || nombre == "Nada"
                || nombre == "Ninguno"
                || nombre == "None")
                && self.coincide(TokenKind::ParenAbrir)
            {
                self.avanzar(); // consume (
                                // Constructor sin argumentos: Ok(), Error(), Algo(), Alguno(), Nada(), Ninguno(), None()
                if self.coincide(TokenKind::ParenCerrar) {
                    self.avanzar(); // consume )
                    return match nombre.as_str() {
                        "Ok" => Ok(Expresion::Ok(Box::new(Expresion::LiteralNulo))),
                        "Error" => Ok(Expresion::Error(Box::new(Expresion::LiteralNulo))),
                        "Algo" | "Some" | "Alguno" => {
                            Ok(Expresion::Algo(Box::new(Expresion::LiteralNulo)))
                        }
                        "Nada" | "Ninguno" | "None" => Ok(Expresion::Ninguno),
                        _ => unreachable!(),
                    };
                }
                let arg = self.parse_expresion()?;
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' después del argumento.",
                )?;
                return match nombre.as_str() {
                    "Ok" => Ok(Expresion::Ok(Box::new(arg))),
                    "Error" => Ok(Expresion::Error(Box::new(arg))),
                    "Algo" | "Some" | "Alguno" => Ok(Expresion::Algo(Box::new(arg))),
                    _ => unreachable!(),
                };
            }

            // Instanciación con BD()
            if nombre == "BD" && self.coincide(TokenKind::ParenAbrir) {
                self.avanzar();
                let argumentos = self.parse_argumentos()?;
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' después de los argumentos de BD.",
                )?;
                return Ok(Expresion::LlamadaFuncion {
                    nombre: "BD".to_string(),
                    argumentos,
                });
            }

            // Para identificadores, manejar llamadas y accesos inline
            return self.parse_llamada_o_acceso(Expresion::Identificador {
                nombre,
                linea: id_linea,
                columna: id_columna,
            });
        }

        if self.coincide(TokenKind::Nuevo) {
            // nuevo como nombre de variable si NO sigue un Identificador (nombre de clase)
            let es_instanciacion = self.pos + 1 < self.tokens.len()
                && matches!(&self.tokens[self.pos + 1].kind, TokenKind::Identificador(_));
            if es_instanciacion {
                return self.parse_instanciacion();
            }
            let (id_linea, id_columna) = (self.peek().linea, self.peek().columna);
            self.avanzar(); // consume 'nuevo' como identificador
            return self.parse_llamada_o_acceso(Expresion::Identificador {
                nombre: "nuevo".to_string(),
                linea: id_linea,
                columna: id_columna,
            });
        }

        if self.coincide(TokenKind::Este) {
            let (id_linea, id_columna) = (self.peek().linea, self.peek().columna);
            self.avanzar();
            let expr = Expresion::Identificador {
                nombre: "self".to_string(),
                linea: id_linea,
                columna: id_columna,
            };
            if self.coincide(TokenKind::Punto) {
                return self.parse_acceso_miembro(expr);
            }
            return Ok(expr);
        }

        // leer() function
        if self.coincide(TokenKind::Leer) {
            self.avanzar();
            self.esperar(TokenKind::ParenAbrir, "Se esperaba '(' después de 'leer'.")?;
            let argumentos = self.parse_argumentos()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los argumentos de 'leer'.",
            )?;
            return Ok(Expresion::LlamadaFuncion {
                nombre: "leer".to_string(),
                argumentos,
            });
        }

        // coincidir (expr) { caso ... }
        if self.coincide(TokenKind::Coincidir) {
            return self.parse_coincidir();
        }

        // escribir() function
        if self.coincide(TokenKind::Escribir) {
            self.avanzar();
            self.esperar(
                TokenKind::ParenAbrir,
                "Se esperaba '(' después de 'escribir'.",
            )?;
            let argumentos = self.parse_argumentos()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los argumentos de 'escribir'.",
            )?;
            return Ok(Expresion::LlamadaFuncion {
                nombre: "escribir".to_string(),
                argumentos,
            });
        }

        // si (cond) { ... } sino { ... } como expresión (layouts condicionales)
        if self.coincide(TokenKind::Si) {
            return self.parse_si_expresion();
        }

        // hilo { ... } como expresión
        if self.coincide(TokenKind::Hilo) {
            self.avanzar();
            self.esperar(TokenKind::LlaveAbrir, "Se esperaba '{' después de 'hilo'.")?;
            let cuerpo = self.parse_bloque()?;
            return Ok(Expresion::Hilo { cuerpo });
        }

        // canal() o canal(capacidad) como expresión
        if self.coincide(TokenKind::Canal) {
            self.avanzar();
            let capacidad = self.parse_canal_capacidad()?;
            return Ok(Expresion::CanalNuevo(capacidad));
        }

        // `resultado` - valor de retorno en postcondiciones (Design by Contract)
        if self.coincide(TokenKind::ResultadoKw) {
            self.avanzar();
            return Ok(Expresion::Resultado);
        }

        // `anterior(expr)` - valor anterior en postcondiciones (Design by Contract)
        if self.coincide(TokenKind::Anterior) {
            self.avanzar();
            self.esperar(
                TokenKind::ParenAbrir,
                "Se esperaba '(' después de 'anterior'",
            )?;
            let expr = self.parse_expresion()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de la expresión en 'anterior'",
            )?;
            return Ok(Expresion::Anterior(Box::new(expr)));
        }

        // Palabras clave que pueden usarse como identificadores en expresiones
        // (soft keywords: son keywords solo en contextos específicos)
        let soft_keywords = [
            TokenKind::Otro,
            TokenKind::Tiempo,
            TokenKind::Implementa,
            TokenKind::Donde,
            TokenKind::Para,
            TokenKind::Caso,
            TokenKind::Seleccionar,
            TokenKind::Retornar,
            TokenKind::Importar,
            TokenKind::Hilo,
            TokenKind::Canal,
            TokenKind::Tipo,
            TokenKind::Funcion,
            TokenKind::Clase,
            TokenKind::Rasgo,
            TokenKind::Externo,
            TokenKind::Prestado,
            TokenKind::Constructor,
            TokenKind::Coincidir,
            TokenKind::Repetir,
            TokenKind::Mientras,
            TokenKind::Variable,
            TokenKind::Constante,
            TokenKind::Nuevo,
            TokenKind::Nulo,
            TokenKind::Verdadero,
            TokenKind::Falso,
            TokenKind::Este,
            TokenKind::Escribir,
            TokenKind::Leer,
            TokenKind::BD,
            TokenKind::Enviar,
            TokenKind::Recibir,
            TokenKind::Unir,
        ];
        for kw in &soft_keywords {
            if self.coincide(kw.clone()) {
                let nombre = self.peek().kind.to_string();
                let (id_linea, id_columna) = (self.peek().linea, self.peek().columna);
                self.avanzar();
                return self.parse_llamada_o_acceso(Expresion::Identificador {
                    nombre,
                    linea: id_linea,
                    columna: id_columna,
                });
            }
        }

        Err(ErrorForja::con_codigo(
            ErrorTipo::ErrorSintactico,
            self.linea_actual(),
            self.columna_actual(),
            Span::punto(self.columna_actual()),
            &format!("Expresión inesperada: {}", self.peek().kind),
            "Revisá la sintaxis de la expresión. ¿Falta un operador o un paréntesis?",
        ))
    }

    /// Verifica si el token actual es el inicio de una expresión interpolada.
    /// Solo incluimos tokens que NUNCA pueden seguir legalmente a un string
    /// literal en código Forja normal (ej: Identificador, Numero, ParenAbrir).
    /// Keywords como Escribir, Leer, Nuevo, etc. NO se incluyen porque pueden
    /// iniciar una nueva declaración después de un string literal.
    pub(crate) fn hay_interpolacion(&self) -> bool {
        if self.es_eof() {
            return false;
        }
        // `->` después de un texto es la flecha de un `caso`, no una
        // interpolación: sin esto `caso "hola" -> { … }` rompía el match.
        if matches!(self.peek().kind, TokenKind::Menos)
            && self
                .peek_siguiente()
                .map(|t| matches!(t.kind, TokenKind::Mayor))
                .unwrap_or(false)
        {
            return false;
        }
        matches!(
            &self.peek().kind,
            TokenKind::Identificador(_)
                | TokenKind::Numero(_)
                | TokenKind::Decimal { .. }
                | TokenKind::LiteralExacto(_, _)
                | TokenKind::Caracter(_)
                | TokenKind::ParenAbrir
                | TokenKind::Menos
                | TokenKind::No
                | TokenKind::Verdadero
                | TokenKind::Falso
                | TokenKind::Nulo
                | TokenKind::Este
                | TokenKind::Texto(_)
        )
    }

    /// Parsea un string interpolado, construyendo una cadena de concatenaciones binarias
    /// "Hola ${nombre}" → Suma(LiteralTexto("Hola "), Identificador("nombre"))
    pub(crate) fn parse_string_interpolado(
        &mut self,
        primer_fragmento: String,
        linea: usize,
    ) -> Result<Expresion, ErrorForja> {
        let mut expr = Expresion::LiteralTexto(primer_fragmento);

        loop {
            // Parsear la expresión interpolada completa (con operadores: +, -, ==, etc.)
            // Usamos parse_expresion() en lugar de parse_expresion_primaria() para
            // soportar expresiones como ${a + b}, ${i + 1}, ${p1 == p2}, ${fn(arg)}, etc.
            let expr_interp = self.parse_expresion()?;
            expr = Expresion::Binaria {
                izquierda: Box::new(expr),
                operador: Operador::Suma,
                derecha: Box::new(expr_interp),
            };

            // Si el siguiente token es otro Texto, concatenarlo también
            if self.coincide(TokenKind::Texto(String::new())) {
                if let TokenKind::Texto(ref s) = self.peek().kind {
                    let s = s.clone();
                    self.avanzar();
                    expr = Expresion::Binaria {
                        izquierda: Box::new(expr),
                        operador: Operador::Suma,
                        derecha: Box::new(Expresion::LiteralTexto(s)),
                    };
                }
                // Si después del Texto hay más interpolación, continuar.
                // Si no, hemos terminado.
                //
                // La continuación tiene que seguir en la misma línea: el lexer
                // deja un `Texto` final (posiblemente vacío) al cerrar el string,
                // y sin este corte el siguiente identificador de la línea
                // siguiente se tomaba como otra interpolación y se fusionaba con
                // la sentencia de abajo (`a = "x=${b}"` + `c = 1` → `… + c = 1`).
                if !self.hay_interpolacion() || self.peek().linea != linea {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(expr)
    }

    /// Parsea postfijo: .miembro, .metodo(), (args), [índice], ? (Try)
    pub(crate) fn parse_postfijo(&mut self, expr: Expresion) -> Result<Expresion, ErrorForja> {
        let mut expr = expr;
        loop {
            if self.coincide(TokenKind::Punto) {
                expr = self.parse_acceso_miembro(expr)?;
            } else if self.coincide(TokenKind::ParenAbrir) {
                self.avanzar();
                let argumentos = self.parse_argumentos()?;
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' después de los argumentos.",
                )?;
                if let Expresion::Identificador { nombre, .. } = expr {
                    expr = Expresion::LlamadaFuncion { nombre, argumentos };
                } else {
                    expr = Expresion::LlamadaFuncion {
                        nombre: "anon".to_string(),
                        argumentos,
                    };
                }
            } else if self.coincide(TokenKind::CorcheteAbrir) {
                expr = self.parse_index(expr)?;
            } else if self.coincide(TokenKind::Interrogacion) && !self.hay_dos_puntos_adelante() {
                self.avanzar(); // consumir ? (operador postfix Try)
                expr = Expresion::Try(Box::new(expr));
            } else {
                break;
            }
        }
        Ok(expr)
    }

    /// Verifica si hay un ':' adelante en el mismo nivel sintáctico para distinguir ternario de Try
    pub(crate) fn hay_dos_puntos_adelante(&self) -> bool {
        let mut idx = self.pos + 1;
        let mut depth_paren = 0;
        let mut depth_corchete = 0;
        let mut depth_llave = 0;
        while idx < self.tokens.len() {
            match &self.tokens[idx].kind {
                TokenKind::PuntoComa | TokenKind::EOF => break,
                TokenKind::DosPuntos
                    if depth_paren == 0 && depth_corchete == 0 && depth_llave == 0 =>
                {
                    return true;
                }
                TokenKind::ParenAbrir => depth_paren += 1,
                TokenKind::ParenCerrar => {
                    if depth_paren == 0 {
                        break;
                    }
                    depth_paren -= 1;
                }
                TokenKind::CorcheteAbrir => depth_corchete += 1,
                TokenKind::CorcheteCerrar => {
                    if depth_corchete == 0 {
                        break;
                    }
                    depth_corchete -= 1;
                }
                TokenKind::LlaveAbrir => depth_llave += 1,
                TokenKind::LlaveCerrar => {
                    if depth_llave == 0 {
                        break;
                    }
                    depth_llave -= 1;
                }
                TokenKind::Coma if depth_paren == 0 && depth_corchete == 0 && depth_llave == 0 => {
                    break;
                }
                _ => {}
            }
            idx += 1;
        }
        false
    }

    /// Parsea acceso por índice: expr[índice]
    pub(crate) fn parse_index(&mut self, objeto: Expresion) -> Result<Expresion, ErrorForja> {
        self.avanzar(); // consume [
        let indice = self.parse_expresion()?;
        self.esperar(
            TokenKind::CorcheteCerrar,
            "Se esperaba ']' después del índice.",
        )?;
        Ok(Expresion::Index {
            objeto: Box::new(objeto),
            indice: Box::new(indice),
        })
    }

    /// Parsea una llamada a función o acceso a miembro después de un identificador
    pub(crate) fn parse_llamada_o_acceso(
        &mut self,
        expr: Expresion,
    ) -> Result<Expresion, ErrorForja> {
        // Saltar parámetros genéricos <Tipo> en llamadas a función: ninguno<Entero>()
        // Verificar si < va seguido de un identificador tipo y >
        if self.coincide(TokenKind::Menor) && self.pos + 3 < self.tokens.len() {
            let puede_ser_generico =
                matches!(&self.tokens[self.pos + 1].kind, TokenKind::Identificador(_))
                    && matches!(&self.tokens[self.pos + 2].kind, TokenKind::Mayor);
            if puede_ser_generico {
                self.avanzar(); // consumir <
                self.avanzar(); // consumir Tipo
                self.avanzar(); // consumir >
            }
        }

        if self.coincide(TokenKind::ParenAbrir) {
            self.avanzar();
            let argumentos = self.parse_argumentos()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los argumentos.",
            )?;

            if let Expresion::Identificador { nombre, .. } = expr {
                return Ok(Expresion::LlamadaFuncion { nombre, argumentos });
            }

            // Si es un acceso a miembro antes de los paréntesis, construimos una llamada a método
            // ej: objeto.metodo(args) -> ya se manejó en parse_acceso_miembro
            return Ok(Expresion::LlamadaFuncion {
                nombre: "anon".to_string(),
                argumentos,
            });
        }

        if self.coincide(TokenKind::Punto) {
            return self.parse_acceso_miembro(expr);
        }

        Ok(expr)
    }

    /// Parsea acceso a miembro: expr.miembro [()] [.miembro...]
    pub(crate) fn parse_acceso_miembro(
        &mut self,
        objeto: Expresion,
    ) -> Result<Expresion, ErrorForja> {
        self.avanzar(); // consume '.'
        let miembro =
            self.esperar_identificador("Se esperaba un nombre de miembro después de '.'.")?;

        // Si sigue (, es llamada a método
        if self.coincide(TokenKind::ParenAbrir) {
            self.avanzar();
            let argumentos = self.parse_argumentos()?;
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los argumentos.",
            )?;

            // Preservar el objeto, método y argumentos en LlamadaMetodo
            return Ok(Expresion::LlamadaMetodo {
                objeto: Box::new(objeto),
                metodo: miembro,
                argumentos,
            });
        }

        let expr = Expresion::AccesoMiembro {
            objeto: Box::new(objeto),
            miembro,
        };

        // Puede haber acceso encadenado (obj.campo.subcampo)
        if self.coincide(TokenKind::Punto) {
            return self.parse_acceso_miembro(expr);
        }

        Ok(expr)
    }

    /// nuevo <Clase>(<argumentos>)  o  nuevo Clase<Tipo>(<argumentos>)
    pub(crate) fn parse_instanciacion(&mut self) -> Result<Expresion, ErrorForja> {
        self.avanzar(); // consume 'nuevo'

        let clase =
            self.esperar_identificador("Se esperaba un nombre de clase después de 'nuevo'.")?;

        // Consumir opcionalmente parámetros genéricos: <Tipo, ...>
        if self.coincide(TokenKind::Menor) {
            self.avanzar(); // consumir <
            let mut depth = 1;
            while depth > 0 && !self.es_eof() {
                match self.peek().kind {
                    TokenKind::Mayor => {
                        self.avanzar();
                        depth -= 1;
                    }
                    TokenKind::Menor => {
                        self.avanzar();
                        depth += 1;
                    }
                    _ => {
                        self.avanzar();
                    }
                }
            }
        }

        if self.coincide(TokenKind::LlaveAbrir) {
            self.avanzar(); // consume '{'
            let mut pares = Vec::new();
            if !self.coincide(TokenKind::LlaveCerrar) {
                loop {
                    let campo = self.esperar_identificador("Se esperaba nombre de campo")?;
                    self.esperar(TokenKind::DosPuntos, "Se esperaba ':' después del campo")?;
                    let val = self.parse_expresion()?;
                    pares.push((Expresion::LiteralTexto(campo), val));
                    if self.coincide(TokenKind::Coma) {
                        self.avanzar();
                        if self.coincide(TokenKind::LlaveCerrar) {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            self.esperar(
                TokenKind::LlaveCerrar,
                "Se esperaba '}' al cerrar struct-literal",
            )?;
            return Ok(Expresion::Instanciacion {
                clase,
                argumentos: vec![Expresion::Mapa(pares)],
            });
        }

        // `nuevo Auto` sin paréntesis = constructor sin argumentos.
        if !self.coincide(TokenKind::ParenAbrir) {
            return Ok(Expresion::Instanciacion {
                clase,
                argumentos: Vec::new(),
            });
        }
        self.avanzar(); // consume '('
        let argumentos = self.parse_argumentos()?;
        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después de los argumentos.",
        )?;

        Ok(Expresion::Instanciacion { clase, argumentos })
    }

    /// Arreglo: [<expr>, <expr>, ...]
    pub(crate) fn parse_arreglo(&mut self) -> Result<Expresion, ErrorForja> {
        self.avanzar(); // consume '['
        let mut elementos = Vec::new();

        if !self.coincide(TokenKind::CorcheteCerrar) {
            loop {
                elementos.push(self.parse_expresion()?);
                if self.coincide(TokenKind::Coma) {
                    self.avanzar();
                    if self.coincide(TokenKind::CorcheteCerrar) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        self.esperar(
            TokenKind::CorcheteCerrar,
            "Se esperaba ']' para cerrar el arreglo.",
        )?;
        Ok(Expresion::Arreglo(elementos))
    }

    /// Mapa literal: {"clave": valor, ...} o {clave = valor, ...} (estilo Lua)
    pub(crate) fn parse_mapa(&mut self) -> Result<Expresion, ErrorForja> {
        self.avanzar(); // consume '{'
        let mut pares = Vec::new();

        if !self.coincide(TokenKind::LlaveCerrar) {
            loop {
                // {clave = valor} (estilo Lua) o {clave: valor}: si el primer
                // token es un identificador y lo sigue '=' o ':', la clave es
                // el texto de ese identificador.
                let clave_sin_comillas = matches!(self.peek().kind, TokenKind::Identificador(_))
                    && {
                        let saved = self.pos;
                        self.avanzar();
                        let es_separador =
                            self.coincide(TokenKind::Igual) || self.coincide(TokenKind::DosPuntos);
                        self.pos = saved; // restaurar
                        es_separador
                    };

                if clave_sin_comillas {
                    self.avanzar(); // consumir identificador
                    let nombre =
                        if let TokenKind::Identificador(n) = &self.tokens[self.pos - 1].kind {
                            n.clone()
                        } else {
                            String::new()
                        };
                    self.avanzar(); // consumir '=' o ':'
                    let valor = self.parse_expresion()?;
                    pares.push((Expresion::LiteralTexto(nombre), valor));
                } else {
                    // Sintaxis normal: {"clave": valor} o {expresion: valor}
                    let clave = self.parse_expresion()?;
                    self.esperar(
                        TokenKind::DosPuntos,
                        "Se esperaba ':' o '=' después de la clave del mapa.",
                    )?;
                    let valor = self.parse_expresion()?;
                    pares.push((clave, valor));
                }

                if self.coincide(TokenKind::Coma) {
                    self.avanzar();
                    if self.coincide(TokenKind::LlaveCerrar) {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        self.esperar(
            TokenKind::LlaveCerrar,
            "Se esperaba '}' para cerrar el mapa.",
        )?;
        Ok(Expresion::Mapa(pares))
    }

    /// Parsea la lista de argumentos: <expr> [, <expr>]*
    pub(crate) fn parse_argumentos(&mut self) -> Result<Vec<Expresion>, ErrorForja> {
        let mut argumentos = Vec::new();

        if self.coincide(TokenKind::ParenCerrar) {
            return Ok(argumentos);
        }

        loop {
            argumentos.push(self.parse_expresion()?);
            if self.coincide(TokenKind::Coma) {
                self.avanzar();
                if self.coincide(TokenKind::ParenCerrar) {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(argumentos)
    }
}

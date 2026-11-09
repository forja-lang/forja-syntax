use super::*;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser {
            tokens,
            pos: 0,
            errores: Vec::new(),
            profundidad: 0,
            profundidad_paren: 0,
            genericos: Vec::new(),
        }
    }

    /// Parsea el programa completo
    pub fn parse(&mut self) -> Result<Programa, Vec<ErrorForja>> {
        let mut declaraciones = Vec::new();

        while !self.es_eof() {
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
                    // No arrastrar genéricos de una declaración que falló a medias.
                    self.genericos.clear();
                    self.sincronizar();
                }
            }
        }

        if self.errores.is_empty() {
            let span = match (
                self.tokens.first().and_then(|t| t.span),
                self.tokens.last().and_then(|t| t.span),
            ) {
                (Some(a), Some(b)) => Some(forja_span::Span {
                    archivo: a.archivo,
                    inicio: a.inicio,
                    fin: b.fin,
                }),
                _ => None,
            };
            let mut programa = Programa {
                spans: Vec::new(),
                nombres: Vec::new(),
                indice_nombres: std::collections::HashMap::new(),
                declaraciones,
                span,
            };
            // `v`/`f` como alias de verdadero/falso cuando no son una variable
            // declarada (ver `booleanos`).
            crate::booleanos::normalizar(&mut programa);
            Ok(programa)
        } else {
            Err(self.errores.clone())
        }
    }

    // ============================================================
    // Parsing de declaraciones
    // ============================================================

    /// Parsea atributos/anotaciones (@derive(Eq), @test, etc.)
    pub(crate) fn parsear_atributos(&mut self) -> Vec<Atributo> {
        let mut atributos = Vec::new();
        while self.coincide(TokenKind::Arroba) {
            self.avanzar(); // consume @
            if let TokenKind::Identificador(nombre) = self.peek().kind.clone() {
                self.avanzar(); // consume nombre del atributo
                let argumentos = if self.coincide(TokenKind::ParenAbrir) {
                    self.avanzar(); // consume (
                    let mut args = Vec::new();
                    while let TokenKind::Identificador(arg) = self.peek().kind.clone() {
                        args.push(arg);
                        self.avanzar();
                        if self.coincide(TokenKind::Coma) {
                            self.avanzar();
                        } else {
                            break;
                        }
                    }
                    let _ = self.esperar(
                        TokenKind::ParenCerrar,
                        "Se esperaba ')' después de argumentos de atributo",
                    );
                    args
                } else {
                    Vec::new()
                };
                atributos.push(Atributo { nombre, argumentos });
            } else {
                break;
            }
        }
        atributos
    }

    /// Parsea una declaración. Retorna None si es EOF.
    pub(crate) fn parsear_declaracion(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        // Prevenir stack overflow en recursión profunda (ej: 100k paréntesis anidados +
        // el bucle de reintento en parsear_declaracion).
        self.profundidad += 1;
        if self.profundidad > MAX_PROFUNDIDAD {
            self.profundidad -= 1;
            return Err(ErrorForja::con_codigo(
                ErrorTipo::DemasiadaAnidacion {
                    max: MAX_PROFUNDIDAD as u32,
                },
                self.linea_actual(),
                self.columna_actual(),
                Span::punto(self.columna_actual()),
                "El parser ha alcanzado el límite de profundidad de recursión",
                "El programa tiene una anidación demasiado profunda. Refactoriza la expresión.",
            ));
        }
        if self.es_eof() {
            self.profundidad -= 1;
            return Ok(None);
        }

        // Recolectar doc comments consecutivos (///) antes de la declaración
        let doc_comment = self.recolectar_doc_comments();

        // Recolectar atributos antes de parsear la declaración
        let atributos = self.parsear_atributos();

        let mut decl = match self.peek().kind {
            TokenKind::Variable | TokenKind::Constante => self.parsear_declaracion_variable(),
            TokenKind::Funcion => {
                // 'funcion' puede ser inicio de declaración de función O llamada a función
                // (ej: funcion() como parámetro callback). Verificamos si el siguiente
                // token es '(' para distinguir: funcion() = llamada, funcion nombre = decl.
                if self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].kind == TokenKind::ParenAbrir
                {
                    match self.parsear_expresion_sentencia() {
                        Ok(r) => Ok(r),
                        Err(_) => {
                            self.avanzar();
                            return self.parsear_declaracion();
                        }
                    }
                } else {
                    self.parsear_funcion()
                }
            }
            TokenKind::Clase => self.parsear_clase(),
            TokenKind::Si => self.parsear_si(),
            TokenKind::Mientras => self.parsear_mientras(),
            TokenKind::Para => self.parsear_para(),
            TokenKind::Repetir => self.parsear_repetir(),
            TokenKind::Cuando => self.parsear_cuando(),
            TokenKind::Retornar => self.parsear_retornar(),
            TokenKind::Romper => {
                self.avanzar();
                Ok(Some(Declaracion::Romper))
            }
            TokenKind::Continuar => {
                self.avanzar();
                Ok(Some(Declaracion::Continuar))
            }
            TokenKind::Importar => {
                // Detectar si es "importar externa" o "importar" normal
                if self.pos + 1 < self.tokens.len() {
                    let next = &self.tokens[self.pos + 1];
                    if next.kind == TokenKind::Externo {
                        self.parsear_importar_externa()
                    } else {
                        self.parsear_importar()
                    }
                } else {
                    self.parsear_importar()
                }
            }
            TokenKind::Externo => self.parsear_funcion(),
            TokenKind::Hilo => self.parsear_hilo(),
            TokenKind::Canal => self.parsear_canal(),
            TokenKind::Seleccionar => self.parsear_seleccionar(),
            TokenKind::Rasgo => self.parsear_rasgo(),
            TokenKind::Tipo => {
                // 'tipo' es soft keyword: `tipo Nombre = …` declara un enum, pero
                // `tipo = valor` es una asignación a una variable llamada `tipo`.
                // Antes se intentaba `parsear_expresion_sentencia`, que no conoce
                // el keyword como identificador: fallaba y partía la asignación
                // en dos sentencias (`tipo` y `valor`), perdiendo el `=`.
                if self.pos + 1 < self.tokens.len()
                    && self.tokens[self.pos + 1].kind == TokenKind::Igual
                {
                    let tok = self.peek().clone();
                    let (linea, columna) = (tok.linea, tok.columna);
                    self.avanzar();
                    self.parsear_post_identificador("tipo".to_string(), linea, columna)
                } else {
                    self.parsear_enum()
                }
            }
            TokenKind::Implementa => self.parsear_implementacion(),
            // `resultado` es keyword (valor de retorno en postcondiciones) pero
            // también se usa como variable corriente: `resultado = resultado + c`
            // en posición de sentencia es una asignación. Antes se partía en dos
            // sentencias y la asignación se perdía en silencio.
            TokenKind::ResultadoKw => {
                let tok = self.peek().clone();
                let (linea, columna) = (tok.linea, tok.columna);
                self.avanzar();
                self.parsear_post_identificador("resultado".to_string(), linea, columna)
            }
            TokenKind::LlaveCerrar => Ok(None), // fin de bloque
            _ => {
                // Si el token arranca una sentencia que el lenguaje reconoce
                // (llamada, asignación, `escribir`, `este`, `nuevo`), su error
                // se propaga. Recuperar en silencio generaba programas vacíos
                // sin diagnóstico: `escribir(hola(` no hacía nada ni avisaba.
                if matches!(
                    self.peek().kind,
                    TokenKind::Identificador(_)
                        | TokenKind::Escribir
                        | TokenKind::Este
                        | TokenKind::Nuevo
                ) {
                    return self.parsear_expresion_sentencia();
                }
                // Token inesperado: intentar como expresión; si falla, saltar y reintentar
                match self.parsear_expresion_sentencia() {
                    Ok(r) => Ok(r),
                    Err(_) => {
                        self.avanzar(); // consumir token problemático
                        return self.parsear_declaracion(); // reintentar
                    }
                }
            }
        };
        self.profundidad -= 1;

        // Asignar atributos a las declaraciones que los soportan
        if !atributos.is_empty() {
            if let Ok(Some(
                Declaracion::Clase {
                    atributos: ref mut a,
                    ..
                }
                | Declaracion::Funcion {
                    atributos: ref mut a,
                    ..
                }
                | Declaracion::Enum {
                    atributos: ref mut a,
                    ..
                },
            )) = decl
            {
                *a = atributos;
            }
        }

        // Asignar doc comment a la declaración (solo para Funcion por ahora)
        if let Some(doc_text) = doc_comment {
            if let Ok(Some(Declaracion::Funcion { ref mut doc, .. })) = decl {
                *doc = Some(doc_text);
            }
        }

        decl
    }

    /// variable <nombre> [: <tipo>] [= <expr>]   → mutable
    /// constante <nombre> [: <tipo>] [= <expr>]  → inmutable
    /// variable a, b = <expr>                     → asignación múltiple
    pub(crate) fn parsear_declaracion_variable(
        &mut self,
    ) -> Result<Option<Declaracion>, ErrorForja> {
        let tok = self.peek().clone();
        let linea = tok.linea;
        let columna = tok.columna;

        // Determinar si es mutable según el keyword
        let mutable = self.coincide(TokenKind::Variable);
        self.avanzar(); // consume 'variable' o 'constante'

        // `variable mut x` (el `mut` es redundante: `variable` ya es mutable).
        // Se acepta porque el LSP y los mensajes de error lo recomiendan.
        if self.coincide(TokenKind::Mut) {
            self.avanzar();
        }

        // Pattern: variable (x, y) = canal()  → asignación múltiple con destructuring (tupla)
        if self.coincide(TokenKind::ParenAbrir) {
            self.avanzar(); // consumir '('
            let mut variables = Vec::new();
            loop {
                let var =
                    self.esperar_identificador("Se esperaba un nombre de variable en el patrón.")?;
                variables.push(var);
                if self.coincide(TokenKind::Coma) {
                    self.avanzar(); // consumir ','
                } else {
                    break;
                }
            }
            self.esperar(
                TokenKind::ParenCerrar,
                "Se esperaba ')' después de los nombres de variable.",
            )?;
            self.esperar(TokenKind::Igual, "Se esperaba '=' después del patrón.")?;
            let valor = self.parsear_expresion()?;
            return Ok(Some(Declaracion::AsignacionMultiple {
                variables,
                mutable,
                valor: Box::new(valor),
            }));
        }

        // Pattern: variable [a, b] = expr  → asignación múltiple con destructuring (arreglo)
        if self.coincide(TokenKind::CorcheteAbrir) {
            self.avanzar(); // consumir '['
            let mut variables = Vec::new();
            loop {
                let var =
                    self.esperar_identificador("Se esperaba un nombre de variable en el patrón.")?;
                variables.push(var);
                if self.coincide(TokenKind::Coma) {
                    self.avanzar(); // consumir ','
                } else {
                    break;
                }
            }
            self.esperar(
                TokenKind::CorcheteCerrar,
                "Se esperaba ']' después de los nombres de variable.",
            )?;
            self.esperar(TokenKind::Igual, "Se esperaba '=' después del patrón.")?;
            let valor = self.parsear_expresion()?;
            return Ok(Some(Declaracion::AsignacionMultiple {
                variables,
                mutable,
                valor: Box::new(valor),
            }));
        }

        let nombre = self.esperar_identificador(if mutable {
            "Se esperaba un nombre de variable después de 'variable'."
        } else {
            "Se esperaba un nombre de constante después de 'constante'."
        })?;

        // Detectar asignación múltiple: variable a, b = expr
        if self.coincide(TokenKind::Coma) {
            let mut variables = vec![nombre];
            while self.coincide(TokenKind::Coma) {
                self.avanzar(); // consumir ','
                let var = self.esperar_identificador("Se esperaba un nombre de variable.")?;
                variables.push(var);
            }
            self.esperar(
                TokenKind::Igual,
                "Se esperaba '=' después de las variables.",
            )?;
            let valor = self.parsear_expresion()?;
            return Ok(Some(Declaracion::AsignacionMultiple {
                variables,
                mutable,
                valor: Box::new(valor),
            }));
        }

        // Tipo opcional
        let tipo = if self.coincide(TokenKind::DosPuntos) {
            self.avanzar();
            Some(self.parsear_tipo()?)
        } else {
            None
        };

        // Valor opcional
        let valor = if self.coincide(TokenKind::Igual) {
            self.avanzar();
            // Un literal con punto asignado a un tipo decimal exacto se construye
            // desde los dígitos del fuente: pasar por f64 mete error binario
            // (12.34 → 12.3399…), que es justo lo que hacía inútil a `Exacto`.
            // shortcut: cubre el literal pelado; para expresiones hace falta
            // resolución dirigida por tipo en el checker (fase 1 del plan).
            if es_decimal_exacto(tipo.as_ref()) {
                match self.literal_decimal_exacto() {
                    Some(expresion) => Some(expresion),
                    None => Some(self.parsear_expresion()?),
                }
            } else {
                Some(self.parsear_expresion()?)
            }
        } else {
            None
        };

        Ok(Some(Declaracion::Variable {
            mutable,
            nombre,
            tipo,
            valor,
            linea,
            columna,
        }))
    }

    /// funcion [asincrona] <nombre>(<parametros>) [-> <tipo>] { <cuerpo> }
    pub(crate) fn parsear_funcion(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        // La declaración puede empezar con `funcion` o con `externo`
        // (`externo printf(...) -> Entero;` es alias de `funcion externa ...;`).
        let empieza_externo = self.coincide(TokenKind::Externo);
        self.avanzar(); // consume 'funcion' o 'externo'

        // Verificar si es asincrónica: funcion asincrona nombre(...)
        let mut asincrona = false;
        if !empieza_externo && self.coincide(TokenKind::Asincrona) {
            asincrona = true;
            self.avanzar(); // consume 'asincrona'
        }

        // Verificar si es externa (FFI): funcion externa nombre(...) ;
        let mut externa = empieza_externo;
        if !empieza_externo && self.coincide(TokenKind::Externo) {
            externa = true;
            self.avanzar(); // consume 'externo' o 'externa'
        }

        let nombre = self.esperar_identificador("Se esperaba el nombre de la función.")?;

        // Parsear parámetros de tipo genérico <T, U> si existen
        let parametros_tipo = self.parsear_parametros_tipo()?;
        let genericos_previos = self.genericos.len();
        self.abrir_genericos(&parametros_tipo);

        self.esperar(
            TokenKind::ParenAbrir,
            "Se esperaba '(' después del nombre de la función.",
        )?;
        let parametros = self.parsear_parametros()?;
        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después de los parámetros.",
        )?;

        // Tipo de retorno opcional
        let tipo_retorno = if self.coincide(TokenKind::Menos) {
            // Podría ser ->
            let col = self.columna_actual();
            self.avanzar();
            if self.coincide(TokenKind::Mayor) {
                self.avanzar();
                Some(self.parsear_tipo()?)
            } else {
                return Err(ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    col,
                    Span::rango(col, col + 1),
                    "Se esperaba '->' para el tipo de retorno.",
                    "Usá '-> Tipo' después de los parámetros para indicar el tipo de retorno.",
                ));
            }
        } else {
            None
        };

        if externa {
            // Función externa termina con ; NO con {}
            self.esperar(
                TokenKind::PuntoComa,
                "Se esperaba ';' al final de la declaración externa.",
            )?;
            let enlace_nombre = Some(nombre.clone());
            self.genericos.truncate(genericos_previos);
            Ok(Some(Declaracion::Funcion {
                nombre,
                parametros_tipo: vec![],
                parametros,
                tipo_retorno,
                cuerpo: vec![],
                externa: true,
                asincrona: false,
                enlace_nombre,
                atributos: vec![],
                doc: None,
                precondiciones: vec![],
                postcondiciones: vec![],
            }))
        } else {
            // Parsear contratos Design by Contract
            let (precondiciones, postcondiciones) = self.parsear_contratos()?;

            self.esperar(
                TokenKind::LlaveAbrir,
                "Se esperaba '{' para el cuerpo de la función.",
            )?;
            let cuerpo = self.parsear_bloque()?;
            self.genericos.truncate(genericos_previos);

            Ok(Some(Declaracion::Funcion {
                nombre,
                parametros_tipo,
                parametros,
                tipo_retorno,
                cuerpo,
                externa: false,
                asincrona,
                enlace_nombre: None,
                atributos: vec![],
                doc: None,
                precondiciones,
                postcondiciones,
            }))
        }
    }

    /// Parsea contratos (requiere/asegura) entre la firma y el cuerpo de función.
    /// Retorna (precondiciones, postcondiciones).
    pub(crate) fn parsear_contratos(
        &mut self,
    ) -> Result<(Vec<Contrato>, Vec<Contrato>), ErrorForja> {
        let mut pre = Vec::new();
        let mut post = Vec::new();

        loop {
            match self.peek().kind {
                TokenKind::Requiere => {
                    self.avanzar();
                    let condicion = self.parsear_expresion()?;
                    let mensaje = self.parsear_mensaje_contrato();
                    pre.push(Contrato { condicion, mensaje });
                }
                TokenKind::Asegura => {
                    self.avanzar();
                    let condicion = self.parsear_expresion()?;
                    let mensaje = self.parsear_mensaje_contrato();
                    post.push(Contrato { condicion, mensaje });
                }
                _ => break,
            }
        }

        Ok((pre, post))
    }

    /// Parsea mensaje opcional de contrato: coma + texto
    pub(crate) fn parsear_mensaje_contrato(&mut self) -> Option<String> {
        if self.coincide(TokenKind::Coma) {
            self.avanzar();
            if let TokenKind::Texto(ref mensaje) = self.peek().kind {
                let mensaje = mensaje.clone();
                self.avanzar();
                return Some(mensaje);
            }
        }
        None
    }

    /// Parsea invariantes de clase (keyword: siempre)
    pub(crate) fn parsear_invariantes(&mut self) -> Result<Vec<Contrato>, ErrorForja> {
        let mut inv = Vec::new();

        loop {
            if self.coincide(TokenKind::Siempre) {
                self.avanzar();
                let condicion = self.parsear_expresion()?;
                let mensaje = self.parsear_mensaje_contrato();
                inv.push(Contrato { condicion, mensaje });
            } else {
                break;
            }
        }

        Ok(inv)
    }

    /// clase <nombre> [<T>] { <campos> <metodos> }
    pub(crate) fn parsear_clase(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'clase'

        let nombre = self.esperar_identificador("Se esperaba el nombre de la clase.")?;

        // Parsear parámetros de tipo genérico <T, U> si existen
        let parametros_tipo = self.parsear_parametros_tipo()?;
        let genericos_previos = self.genericos.len();
        self.abrir_genericos(&parametros_tipo);

        // Parsear invariantes de clase (siempre)
        let mut invariantes = self.parsear_invariantes()?;

        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el cuerpo de la clase.",
        )?;

        let mut campos = Vec::new();
        let mut metodos = Vec::new();

        while !self.coincide(TokenKind::LlaveCerrar) && !self.es_eof() {
            if self.coincide(TokenKind::Constructor) || self.coincide(TokenKind::Funcion) {
                metodos.push(self.parsear_metodo_en_clase()?);
            } else if self.coincide(TokenKind::Siempre) {
                // los `siempre` también se aceptan **dentro** del cuerpo.
                // Antes caían en `parsear_campo_en_clase` y se descartaban en
                // silencio (el invariante no se registraba y el parser no
                // avisaba).
                invariantes.extend(self.parsear_invariantes()?);
            } else {
                // Es una declaración de campo
                self.parsear_campo_en_clase(&mut campos)?;
            }
        }

        self.esperar(
            TokenKind::LlaveCerrar,
            "Se esperaba '}' para cerrar la clase.",
        )?;
        self.genericos.truncate(genericos_previos);

        Ok(Some(Declaracion::Clase {
            nombre,
            parametros_tipo,
            campos,
            metodos,
            atributos: vec![],
            invariantes,
        }))
    }

    /// rasgo <nombre> [<T>] [: RasgoPadre] { funcion nombre(params) [-> Tipo] ... }
    pub(crate) fn parsear_rasgo(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'rasgo'

        let nombre = self.esperar_identificador("Se esperaba el nombre del rasgo.")?;

        // Parámetros de tipo genérico opcionales: rasgo Nombre<T>
        let parametros_tipo = if self.coincide(TokenKind::Menor) {
            self.parsear_parametros_tipo()?
        } else {
            Vec::new()
        };
        let genericos_previos = self.genericos.len();
        self.abrir_genericos(&parametros_tipo);

        // Herencia opcional: rasgo Hijo : Padre
        if self.coincide(TokenKind::DosPuntos) {
            self.avanzar(); // consume ':'
            let _padre = self.esperar_identificador("Se esperaba nombre del rasgo padre.")?;
            // Por ahora solo ignoramos el rasgo padre (soporte futuro)
        }

        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para iniciar cuerpo del rasgo.",
        )?;

        let mut metodos = Vec::new();
        while !self.coincide(TokenKind::LlaveCerrar) && !self.es_eof() {
            // Cada método en rasgo: funcion nombre(params) [-> Tipo]
            if self.coincide(TokenKind::Funcion) {
                self.avanzar(); // consume 'funcion'
                let nombre_metodo =
                    self.esperar_identificador("Se esperaba nombre del método en rasgo.")?;
                self.esperar(
                    TokenKind::ParenAbrir,
                    "Se esperaba '(' después del nombre del método.",
                )?;
                let parametros = self.parsear_parametros()?;
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' después de los parámetros.",
                )?;

                let tipo_retorno = if self.coincide(TokenKind::Menos) {
                    let col = self.columna_actual();
                    self.avanzar();
                    if self.coincide(TokenKind::Mayor) {
                        self.avanzar();
                        Some(self.parsear_tipo()?)
                    } else {
                        return Err(ErrorForja::con_codigo(
                            ErrorTipo::ErrorSintactico,
                            self.linea_actual(),
                            col,
                            Span::rango(col, col + 1),
                            "Se esperaba '->' para el tipo de retorno.",
                            "Usá '-> Tipo' después de los parámetros.",
                        ));
                    }
                } else {
                    None
                };

                metodos.push(FirmaMetodo {
                    nombre: nombre_metodo,
                    parametros,
                    tipo_retorno,
                });

                // Si el método tiene cuerpo (implementación por defecto), saltarlo
                if self.coincide(TokenKind::LlaveAbrir) {
                    // Consumir todo hasta la } correspondiente
                    let mut depth = 1;
                    self.avanzar(); // consume '{'
                    while depth > 0 && !self.es_eof() {
                        if self.coincide(TokenKind::LlaveAbrir) {
                            depth += 1;
                        } else if self.coincide(TokenKind::LlaveCerrar) {
                            depth -= 1;
                        }
                        self.avanzar();
                    }
                }
            }
            // Si no es 'funcion', avanzar para evitar bucle infinito
            if !self.coincide(TokenKind::Funcion) && !self.coincide(TokenKind::LlaveCerrar) {
                self.avanzar();
            }
        }
        self.esperar(
            TokenKind::LlaveCerrar,
            "Se esperaba '}' para cerrar el rasgo.",
        )?;
        self.genericos.truncate(genericos_previos);
        Ok(Some(Declaracion::Rasgo { nombre, metodos }))
    }

    /// tipo <nombre> = Variante1 | Variante2(Tipo, Tipo) | Variante3
    pub(crate) fn parsear_enum(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'tipo'

        let nombre =
            self.esperar_identificador("Se esperaba nombre del tipo después de 'tipo'.")?;
        self.esperar(
            TokenKind::Igual,
            "Se esperaba '=' después del nombre del tipo.",
        )?;

        let mut variantes = Vec::new();
        loop {
            let var_nombre = self.esperar_identificador("Se esperaba nombre de variante.")?;
            // Verificar si la variante tiene datos asociados: Nombre(Tipo, Tipo)
            let tipos = if self.coincide(TokenKind::ParenAbrir) {
                self.avanzar();
                let mut ts = Vec::new();
                loop {
                    ts.push(self.parsear_tipo()?);
                    if self.coincide(TokenKind::Coma) {
                        self.avanzar();
                    } else {
                        break;
                    }
                }
                self.esperar(
                    TokenKind::ParenCerrar,
                    "Se esperaba ')' después de los tipos de la variante.",
                )?;
                ts
            } else {
                Vec::new()
            };
            variantes.push(Variante {
                nombre: var_nombre,
                tipos,
            });

            if self.coincide(TokenKind::Pipe) {
                self.avanzar(); // consumir |
            } else {
                break;
            }
        }

        // Los atributos ya fueron recolectados antes de parsear_declaracion()
        Ok(Some(Declaracion::Enum {
            nombre,
            variantes,
            atributos: vec![],
        }))
    }

    /// implementa <rasgo>[<T>] para <clase> { funcion nombre(params) [-> Tipo] { ... } ... }
    pub(crate) fn parsear_implementacion(&mut self) -> Result<Option<Declaracion>, ErrorForja> {
        self.avanzar(); // consume 'implementa'
        let rasgo_nombre = self.esperar_identificador("Se esperaba nombre del rasgo.")?;

        // Parámetros de tipo genérico opcionales en el rasgo: implementa Comparador<Entero> para ...
        if self.coincide(TokenKind::Menor) {
            self.parsear_parametros_tipo()?; // consumir <Entero> o similares
        }

        // consumir 'para'
        if !self.coincide(TokenKind::Para) {
            return Err(ErrorForja::con_codigo(
                ErrorTipo::ErrorSintactico,
                self.linea_actual(),
                self.columna_actual(),
                Span::punto(self.columna_actual()),
                "Se esperaba 'para' después del nombre del rasgo.",
                "Usá: implementa Rasgo para Clase { ... }",
            ));
        }
        self.avanzar(); // consume 'para'

        let clase_nombre = self.esperar_identificador("Se esperaba nombre de la clase.")?;
        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el cuerpo de la implementación.",
        )?;

        let mut metodos = Vec::new();
        while !self.coincide(TokenKind::LlaveCerrar) && !self.es_eof() {
            if self.coincide(TokenKind::Funcion) || self.coincide(TokenKind::Constructor) {
                metodos.push(self.parsear_metodo_en_clase()?);
            }
            // Si no es 'funcion', avanzar para evitar bucle infinito
            if !self.coincide(TokenKind::Funcion)
                && !self.coincide(TokenKind::Constructor)
                && !self.coincide(TokenKind::LlaveCerrar)
            {
                self.avanzar();
            }
        }
        self.esperar(
            TokenKind::LlaveCerrar,
            "Se esperaba '}' para cerrar la implementación.",
        )?;
        Ok(Some(Declaracion::Implementacion {
            rasgo_nombre,
            clase_nombre,
            metodos,
        }))
    }

    /// Parsea un campo dentro de una clase: <nombre> [: <tipo>]
    pub(crate) fn parsear_campo_en_clase(
        &mut self,
        campos: &mut Vec<VariableClase>,
    ) -> Result<(), ErrorForja> {
        let nombre = self.esperar_identificador("Se esperaba un nombre de campo en la clase.")?;

        // Tipo opcional: campo: Tipo
        let tipo = if self.coincide(TokenKind::DosPuntos) {
            self.avanzar();
            Some(self.parsear_tipo()?)
        } else {
            None
        };

        campos.push(VariableClase { nombre, tipo });
        Ok(())
    }

    /// Parsea un método dentro de una clase o implementación
    pub(crate) fn parsear_metodo_en_clase(&mut self) -> Result<Metodo, ErrorForja> {
        let es_constructor = self.coincide(TokenKind::Constructor);
        if es_constructor {
            self.avanzar(); // consume 'constructor'
        } else {
            self.avanzar(); // consume 'funcion'
        }

        let nombre = if es_constructor {
            String::new() // el constructor se llamará "nuevo"
        } else {
            self.esperar_identificador("Se esperaba el nombre del método.")?
        };

        self.esperar(
            TokenKind::ParenAbrir,
            "Se esperaba '(' después del nombre del método.",
        )?;
        let parametros = self.parsear_parametros()?;
        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después de los parámetros.",
        )?;

        // Tipo de retorno opcional: -> Tipo
        let tipo_retorno = if self.coincide(TokenKind::Menos) {
            let col = self.columna_actual();
            self.avanzar();
            if self.coincide(TokenKind::Mayor) {
                self.avanzar();
                Some(self.parsear_tipo()?)
            } else {
                return Err(ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    col,
                    Span::rango(col, col + 1),
                    "Se esperaba '->' para el tipo de retorno.",
                    "Usá '-> Tipo' después de los parámetros.",
                ));
            }
        } else {
            None
        };

        // Parsear contratos Design by Contract
        let (precondiciones, postcondiciones) = self.parsear_contratos()?;

        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para el cuerpo del método.",
        )?;
        let cuerpo = self.parsear_bloque()?;

        Ok(Metodo {
            nombre: if es_constructor {
                "nuevo".to_string()
            } else {
                nombre
            },
            parametros,
            tipo_retorno,
            cuerpo,
            precondiciones,
            postcondiciones,
        })
    }

    /// Parsea la lista de parámetros: [prestado] <nombre> [, [prestado] <nombre>]*
    pub(crate) fn parsear_parametros(&mut self) -> Result<Vec<Parametro>, ErrorForja> {
        let mut parametros = Vec::new();

        if self.coincide(TokenKind::ParenCerrar) {
            return Ok(parametros);
        }

        loop {
            let prestado = self.coincide(TokenKind::Prestado);
            if prestado {
                self.avanzar();
            }

            let mutable = if self.coincide(TokenKind::Mut) {
                self.avanzar();
                true
            } else {
                false
            };

            let nombre = self.esperar_identificador("Se esperaba un nombre de parámetro.")?;

            // Consumir elipsis (...) si existe para soportar stubs variádicos
            if self.coincide(TokenKind::Punto) {
                while self.coincide(TokenKind::Punto) {
                    self.avanzar();
                }
            }

            // Tipo opcional
            let tipo = if self.coincide(TokenKind::DosPuntos) {
                self.avanzar();
                Some(self.parsear_tipo()?)
            } else {
                None
            };

            parametros.push(Parametro {
                nombre,
                prestado,
                mutable,
                tipo,
            });

            if self.coincide(TokenKind::Coma) {
                self.avanzar();
                if self.coincide(TokenKind::ParenCerrar) {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(parametros)
    }

    /// Registra los parámetros genéricos declarados en `<...>` para esta declaración.
    pub(crate) fn abrir_genericos(&mut self, parametros: &[ParametroTipo]) {
        self.genericos
            .extend(parametros.iter().map(|p| p.nombre.clone()));
    }

    /// Parsea parámetros de tipo genérico: <T, U, V>
    /// Retorna vacío si no hay <>
    pub(crate) fn parsear_parametros_tipo(&mut self) -> Result<Vec<ParametroTipo>, ErrorForja> {
        let mut params = Vec::new();
        if self.coincide(TokenKind::Menor) {
            self.avanzar(); // consume <
            loop {
                let nombre =
                    self.esperar_identificador("Se esperaba nombre de parámetro de tipo")?;
                params.push(ParametroTipo { nombre });
                if self.coincide(TokenKind::Coma) {
                    self.avanzar();
                    if self.coincide(TokenKind::Mayor) {
                        break;
                    }
                } else {
                    break;
                }
            }
            self.esperar(
                TokenKind::Mayor,
                "Se esperaba > para cerrar parámetros de tipo",
            )?;
        }
        Ok(params)
    }
}

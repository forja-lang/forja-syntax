use crate::ast::*;
use crate::error::{ErrorForja, ErrorTipo, Span};
use crate::token::{Token, TokenKind};

pub mod control;
pub mod declaraciones;
pub mod expresiones;
pub mod primarias;
pub mod tipos;

/// Parser recursivo descendente para Forja (fa)
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errores: Vec<ErrorForja>,
    /// Profundidad de recursión global del parser (previene stack overflow).
    /// Se incrementa al entrar a parsear_declaracion y se decrementa al salir.
    profundidad: usize,
    /// Profundidad de anidamiento de paréntesis (para mensaje de error específico).
    profundidad_paren: usize,
    /// Parámetros genéricos de tipo visibles en este punto (`funcion f<T>`, `clase C<T>`).
    genericos: Vec<String>,
}

/// Límite máximo de profundidad de recursión global del parser.
/// Cada llamada recursiva en parsear_declaracion consume ~2KB de stack en debug mode.
/// Con 2MB de stack, 512 es seguro. Lo ponemos en 256 para ser conservadores.
const MAX_PROFUNDIDAD: usize = 256;

/// Límite máximo de anidamiento de paréntesis dentro de una expresión.
const MAX_ANIDAMIENTO_PAREN: usize = 32;

impl Parser {
    /// Obtiene el token siguiente al actual sin consumirlo
    fn peek_siguiente(&self) -> Option<&Token> {
        if self.pos + 1 < self.tokens.len() {
            Some(&self.tokens[self.pos + 1])
        } else {
            None
        }
    }

    /// Avanza al siguiente token y retorna el token anterior (clonado)
    fn avanzar(&mut self) -> Token {
        let token = self.peek().clone();
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        token
    }

    /// Verifica si es EOF
    fn es_eof(&self) -> bool {
        self.pos >= self.tokens.len() || self.peek().kind == TokenKind::EOF
    }

    /// Espera y consume un token de cierto tipo, o lanza error
    fn esperar(&mut self, kind: TokenKind, mensaje: &str) -> Result<(), ErrorForja> {
        if self.coincide(kind) {
            self.avanzar();
            Ok(())
        } else {
            Err(ErrorForja::con_codigo(
                ErrorTipo::ErrorSintactico,
                self.linea_actual(),
                self.columna_actual(),
                Span::punto(self.columna_actual()),
                mensaje,
                "Se esperaba un token específico.",
            ))
        }
    }

    /// Espera un identificador y devuelve su nombre.
    /// También acepta palabras clave (soft keywords) que pueden usarse como nombres.
    fn esperar_identificador(&mut self, mensaje: &str) -> Result<String, ErrorForja> {
        let token = self.peek().kind.clone();
        let nombre = match &token {
            TokenKind::Identificador(name) => name.clone(),
            // Soft keywords: todos los keywords pueden usarse como nombres
            TokenKind::Este => "self".to_string(),
            TokenKind::TipoEntero => "Entero".to_string(),
            TokenKind::TipoDecimal => "Decimal".to_string(),
            TokenKind::TipoTexto => "Texto".to_string(),
            TokenKind::TipoBooleano => "Booleano".to_string(),
            TokenKind::TipoExacto => "Exacto".to_string(),
            TokenKind::Enviar => "enviar".to_string(),
            TokenKind::Recibir => "recibir".to_string(),
            TokenKind::Unir => "unir".to_string(),
            // Cualquier otro keyword se convierte a string y se usa como nombre
            _ => {
                let s = format!("{}", token);
                if s.starts_with('<')
                    || s.starts_with('(')
                    || s.starts_with('{')
                    || s.starts_with('[')
                    || s.starts_with('"')
                    || token == TokenKind::EOF
                {
                    return Err(ErrorForja::con_codigo(
                        ErrorTipo::ErrorSintactico,
                        self.linea_actual(),
                        self.columna_actual(),
                        Span::punto(self.columna_actual()),
                        mensaje,
                        "Se esperaba un nombre (identificador).",
                    ));
                }
                s
            }
        };
        self.avanzar();
        Ok(nombre)
    }

    fn linea_actual(&self) -> usize {
        self.peek().linea
    }

    fn columna_actual(&self) -> usize {
        self.peek().columna
    }

    fn es_inicio_expresion(&self) -> bool {
        if self.es_eof() {
            return false;
        }
        matches!(
            self.peek().kind,
            TokenKind::Identificador(_)
                | TokenKind::Numero(_)
                | TokenKind::Decimal { .. }
                | TokenKind::LiteralExacto(_, _)
                | TokenKind::Texto(_)
                | TokenKind::Caracter(_)
                | TokenKind::Verdadero
                | TokenKind::Falso
                | TokenKind::Nulo
                | TokenKind::ParenAbrir
                | TokenKind::CorcheteAbrir
                | TokenKind::Nuevo
                | TokenKind::Este
                | TokenKind::Escribir
                | TokenKind::Coincidir
                | TokenKind::No
                | TokenKind::Menos
                | TokenKind::Amp
                | TokenKind::Tipo
                // `resultado` y `anterior(...)` empiezan una expresión: sin
                // esto, `retornar resultado` se partía en `retornar` + una
                // sentencia aparte y la función devolvía nulo.
                | TokenKind::ResultadoKw
                | TokenKind::Anterior
        ) || (self.coincide(TokenKind::Funcion)
            // `func(x) { … }` (closure literal) empieza una expresión; una
            // declaración `funcion nombre(` no.
            && self
                .peek_siguiente()
                .is_some_and(|t| t.kind == TokenKind::ParenAbrir))
    }

    // ============================================================
    // Parsing de match (coincidir)
    // ============================================================

    /// coincidir (expr) { caso patron1 | patron2 { cuerpo } ... }
    fn parsear_coincidir(&mut self) -> Result<Expresion, ErrorForja> {
        self.avanzar(); // consume 'coincidir'

        self.esperar(
            TokenKind::ParenAbrir,
            "Se esperaba '(' después de 'coincidir'.",
        )?;
        let expr = self.parsear_expresion()?;
        self.esperar(
            TokenKind::ParenCerrar,
            "Se esperaba ')' después de la expresión a coincidir.",
        )?;

        self.esperar(
            TokenKind::LlaveAbrir,
            "Se esperaba '{' para los brazos del match.",
        )?;
        let brazos = self.parsear_brazos_de_coincidencia()?;

        Ok(Expresion::Coincidir {
            expr: Box::new(expr),
            brazos,
        })
    }

    /// Parsea los brazos de un match: caso patron1 | patron2 { cuerpo } ...
    /// Soporta tanto sintaxis con -> (caso Patron -> { ... }) como sin él (caso Patron { ... }).
    fn parsear_brazos_de_coincidencia(&mut self) -> Result<Vec<BrazoMatch>, ErrorForja> {
        let mut brazos = Vec::new();

        while !self.coincide(TokenKind::LlaveCerrar) && !self.es_eof() {
            if !self.coincide(TokenKind::Caso) {
                return Err(ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    self.columna_actual(),
                    Span::punto(self.columna_actual()),
                    "Se esperaba 'caso' dentro del match.",
                    "Usá: caso patron -> { cuerpo } o caso patron1 | patron2 { cuerpo }",
                ));
            }
            self.avanzar(); // consume 'caso'

            let patrones = self.parsear_patrones_con_barra()?;

            // `caso Patron si (cond)` — guarda: no soportada todavía.
            // Se rechaza con un mensaje claro en vez de dejar que el `si` se
            // confunda con el cuerpo del brazo y el patrón pierda su binding.
            // El error se registra acá además de devolverse: la recuperación de
            // `parsear_declaracion` descarta los errores de un reintento, y sin
            // esto el usuario veía un error del checker en vez de este.
            if self.coincide(TokenKind::Si) {
                let error = ErrorForja::con_codigo(
                    ErrorTipo::ErrorSintactico,
                    self.linea_actual(),
                    self.columna_actual(),
                    Span::punto(self.columna_actual()),
                    "Las guardas en patrones todavía no están soportadas.",
                    "Poné la condición dentro del cuerpo del caso: caso Circulo(r) -> { si !(r > 0) { ... } }",
                );
                self.errores.push(error.clone());
                return Err(error);
            }

            // Opcionalmente consumir -> (sintaxis existente: caso Patron -> { ... })
            if self.coincide(TokenKind::Menos) {
                let col = self.columna_actual();
                self.avanzar();
                if self.coincide(TokenKind::Mayor) {
                    self.avanzar();
                } else {
                    return Err(ErrorForja::con_codigo(
                        ErrorTipo::ErrorSintactico,
                        self.linea_actual(),
                        col,
                        Span::rango(col, col + 1),
                        "Se esperaba '->' después del patrón.",
                        "Usá: caso patron -> { cuerpo }",
                    ));
                }
            }

            // Si hay {, parsear bloque completo; si no, parsear expresión simple
            let cuerpo = if self.coincide(TokenKind::LlaveAbrir) {
                self.avanzar(); // consume {
                self.parsear_bloque()? // parsear_bloque consume el }
            } else if self.coincide(TokenKind::Retornar) {
                // Si la expresión es 'retornar', parsear la declaración completa
                let decl = self.parsear_declaracion()?.ok_or_else(|| {
                    ErrorForja::con_codigo(
                        ErrorTipo::ErrorSintactico,
                        self.linea_actual(),
                        self.columna_actual(),
                        Span::punto(self.columna_actual()),
                        "Se esperaba una declaración después de 'retornar'.",
                        "Usá: retornar expresión",
                    )
                })?;
                vec![decl]
            } else {
                // Expresión simple después de ->
                let expr = self.parsear_expresion()?;
                vec![Declaracion::Expresion(expr)]
            };

            // Desugaring: crear un brazo separado para cada patrón, todos con el MISMO cuerpo
            for patron in patrones {
                brazos.push(BrazoMatch {
                    patron,
                    cuerpo: cuerpo.clone(),
                });
            }
        }

        // Consumir la llave de cierre del match
        if self.coincide(TokenKind::LlaveCerrar) {
            self.avanzar();
        }

        Ok(brazos)
    }

    /// Parsea uno o más patrones separados por | (pipe)
    /// Ej: 1 | 2 | 3  → [Literal(1), Literal(2), Literal(3)]
    fn parsear_patrones_con_barra(&mut self) -> Result<Vec<Patron>, ErrorForja> {
        let mut patrones = Vec::new();
        loop {
            let patron = self.parsear_patron()?;
            patrones.push(patron);
            if self.coincide(TokenKind::Pipe) {
                self.avanzar(); // consume |
            } else {
                break;
            }
        }
        Ok(patrones)
    }

    /// Parsea un patrón individual:
    /// - NombreVariante       → Constructor(nombre, [])
    /// - NombreVariante(p1, p2) → Constructor(nombre, [p1, p2])
    /// - _                    → Ignorar
    /// - literal (42, "hola", verdadero, etc.) → Literal(expr)
    fn parsear_patron(&mut self) -> Result<Patron, ErrorForja> {
        match &self.peek().kind {
            TokenKind::Identificador(nombre) => {
                let nombre = nombre.clone();
                self.avanzar();

                // _ es el patrón ignorar
                if nombre == "_" {
                    return Ok(Patron::Ignorar);
                }

                // Si sigue (, es constructor con parámetros: Nombre(p1, p2)
                if self.coincide(TokenKind::ParenAbrir) {
                    self.avanzar(); // consume (
                    let mut subpatrones = Vec::new();
                    if !self.coincide(TokenKind::ParenCerrar) {
                        loop {
                            subpatrones.push(self.parsear_patron()?);
                            if self.coincide(TokenKind::Coma) {
                                self.avanzar();
                                if self.coincide(TokenKind::ParenCerrar) {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                    self.esperar(
                        TokenKind::ParenCerrar,
                        "Se esperaba ')' después de los subpatrones del constructor.",
                    )?;
                    Ok(Patron::Constructor(
                        normalizar_constructor_patron(nombre),
                        subpatrones,
                    ))
                } else {
                    // Distinguir entre variable y constructor según la primera letra:
                    // - Minúscula o empieza con _ → Variable (bindea el valor)
                    // - Mayúscula → Constructor (variante de enum sin datos)
                    if nombre.starts_with(char::is_uppercase) {
                        Ok(Patron::Constructor(
                            normalizar_constructor_patron(nombre),
                            vec![],
                        ))
                    } else {
                        Ok(Patron::Variable(nombre))
                    }
                }
            }
            TokenKind::Numero(_)
            | TokenKind::Decimal { .. }
            | TokenKind::LiteralExacto(_, _)
            | TokenKind::Texto(_)
            | TokenKind::Verdadero
            | TokenKind::Falso
            | TokenKind::Nulo => {
                let expr = self.parsear_expresion_primaria()?;
                Ok(Patron::Literal(expr))
            }
            TokenKind::Menos => {
                // Número negativo como patrón: -5
                let expr = self.parsear_expresion_unaria()?;
                Ok(Patron::Literal(expr))
            }
            _ => Err(ErrorForja::con_codigo(
                ErrorTipo::ErrorSintactico,
                self.linea_actual(),
                self.columna_actual(),
                Span::punto(self.columna_actual()),
                &format!(
                    "Se esperaba un patrón, pero se encontró: {}",
                    self.peek().kind
                ),
                "Usá un nombre de variante, un literal, o '_' para ignorar.",
            )),
        }
    }
}

// ============================================================
// Funciones auxiliares para parsear_seleccionar
// ============================================================

/// Extrae un número entero de una expresión literal
fn extraer_numero(expr: &Expresion) -> u64 {
    if let Expresion::LiteralNumero(n) = expr {
        *n as u64
    } else {
        0
    }
}

/// Determina si una ruta es absoluta (Unix: /inicio, Windows: C:\ o C:/)
fn es_ruta_absoluta(ruta: &str) -> bool {
    if ruta.starts_with('/') {
        return true;
    }
    // Windows: letra de unidad seguida de :\ o :/
    if ruta.len() >= 3 {
        let bytes = ruta.as_bytes();
        if bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'/' || bytes[2] == b'\\')
        {
            return true;
        }
    }
    false
}

/// ¿El tipo declarado es un decimal exacto (no binario)?
/// Los literales `variable x: Exacto = 12.34` deben construirse desde los dígitos
/// del fuente, no desde el f64 que también trae el token.
fn es_decimal_exacto(tipo: Option<&Tipo>) -> bool {
    matches!(tipo, Some(Tipo::Exacto))
}

impl Parser {
    /// Consume `[\-] DECIMAL` y devuelve el literal exacto con los dígitos del
    /// fuente (el signo se aplica al coeficiente). Devuelve `None` sin consumir
    /// nada si el próximo token no es un literal con punto.
    fn literal_decimal_exacto(&mut self) -> Option<Expresion> {
        let negativo = self.coincide(TokenKind::Menos);
        let pos_inicial = self.pos;
        if negativo {
            self.avanzar();
        }
        match self.peek().kind.clone() {
            TokenKind::Decimal { coeff, escala, .. } => {
                self.avanzar();
                let coeff = if negativo { -coeff } else { coeff };
                Some(Expresion::LiteralExacto(coeff, escala))
            }
            _ => {
                self.pos = pos_inicial;
                None
            }
        }
    }
}

/// Nombre canónico de una variante usada en un patrón.
///
/// `Opcion<T>` se declara en el prelude como `Alguno(T) | Ninguno`, pero los
/// ejemplos usan `Algo(...)` / `Nada()`: sin normalizar, `caso Algo(v)` no
/// matcheaba en runtime (devolvía Nulo) y el chequeo de exhaustividad lo veía
/// como una variante distinta.
fn normalizar_constructor_patron(nombre: String) -> String {
    match nombre.as_str() {
        "Algo" | "Some" => "Alguno".to_string(),
        "Nada" | "None" => "Ninguno".to_string(),
        _ => nombre,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parsear_fuente(fuente: &str) -> Result<Programa, Vec<ErrorForja>> {
        let mut lexer = Lexer::new(fuente);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser::new(tokens);
        parser.parse()
    }

    #[test]
    fn parsea_variable() {
        let prog = parsear_fuente("variable x = 5").unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::Variable {
                mutable,
                nombre,
                valor,
                ..
            } => {
                assert!(mutable); // 'variable' = mutable
                assert_eq!(nombre, "x");
                assert!(valor.is_some());
            }
            _ => panic!("Se esperaba Declaracion::Variable"),
        }
    }

    #[test]
    fn parsea_constante() {
        let prog = parsear_fuente("constante x = 10").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Variable {
                mutable, nombre, ..
            } => {
                assert!(!mutable); // 'constante' = inmutable
                assert_eq!(nombre, "x");
            }
            _ => panic!("Se esperaba Declaracion::Variable"),
        }
    }

    #[test]
    fn test_tipo_como_variable_en_asignacion() {
        // `tipo` es soft keyword: `tipo = valor` es una asignación, no un enum.
        // Antes se partía en dos sentencias y se perdía el `=`.
        let prog = parsear_fuente("funcion f() {\n    tipo = \"X\"\n}").unwrap();
        let cuerpo = match &prog.declaraciones[0] {
            Declaracion::Funcion { cuerpo, .. } => cuerpo,
            otra => panic!("se esperaba una función, llegó: {otra:?}"),
        };
        assert_eq!(cuerpo.len(), 1);
        match &cuerpo[0] {
            Declaracion::Asignacion { nombre, valor, .. } => {
                assert_eq!(nombre, "tipo");
                assert!(matches!(**valor, Expresion::LiteralTexto(_)));
            }
            otra => panic!("se esperaba Declaracion::Asignacion, llegó: {otra:?}"),
        }
    }

    #[test]
    fn test_tipo_de_una_letra_no_es_generico() {
        // `tipo F =...` declaraba el tipo, pero `f: F` se parseaba como
        // parámetro genérico y la exhaustividad no veía el enum.
        let prog =
            parsear_fuente("tipo F = A | B\nfuncion usar(f: F) -> Entero { retornar 1 }").unwrap();
        match &prog.declaraciones[1] {
            Declaracion::Funcion { parametros, .. } => {
                assert!(
                    matches!(parametros[0].tipo, Some(Tipo::Clase(ref n)) if n == "F"),
                    "se esperaba Clase(\"F\"), llegó: {:?}",
                    parametros[0].tipo
                );
            }
            otra => panic!("se esperaba una función, llegó: {otra:?}"),
        }
        // Pero un genérico declarado en <...> sigue siendo Parametro.
        let prog = parsear_fuente("funcion id<T>(x: T) -> T { retornar x }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Funcion {
                parametros,
                tipo_retorno,
                ..
            } => {
                assert!(matches!(parametros[0].tipo, Some(Tipo::Parametro(ref n)) if n == "T"));
                assert!(matches!(tipo_retorno, Some(Tipo::Parametro(ref n)) if n == "T"));
            }
            otra => panic!("se esperaba una función, llegó: {otra:?}"),
        }
        // `T` sin declarar en `<...>` tampoco es genérico por convención: es un tipo.
        let prog = parsear_fuente("funcion usar(x: T) -> T { retornar x }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Funcion {
                parametros,
                tipo_retorno,
                ..
            } => {
                assert!(matches!(parametros[0].tipo, Some(Tipo::Clase(ref n)) if n == "T"));
                assert!(matches!(tipo_retorno, Some(Tipo::Clase(ref n)) if n == "T"));
            }
            otra => panic!("se esperaba una función, llegó: {otra:?}"),
        }
        // El ámbito genérico no se filtra fuera de la declaración que lo abrió.
        let prog = parsear_fuente("funcion f<T>(x: T) { }\nfuncion g(y: T) { }").unwrap();
        match &prog.declaraciones[1] {
            Declaracion::Funcion { parametros, .. } => {
                assert!(matches!(parametros[0].tipo, Some(Tipo::Clase(ref n)) if n == "T"));
            }
            otra => panic!("se esperaba una función, llegó: {otra:?}"),
        }
        // Y sí alcanza a los métodos del cuerpo de la clase y a las firmas del rasgo.
        let prog = parsear_fuente("clase C<T> { funcion m(x: T) -> T { retornar x } }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Clase { metodos, .. } => {
                assert!(matches!(
                    metodos[0].parametros[0].tipo,
                    Some(Tipo::Parametro(ref n)) if n == "T"
                ));
            }
            otra => panic!("se esperaba una clase, llegó: {otra:?}"),
        }
        let prog = parsear_fuente("rasgo R<T> { funcion f(x: T) -> T }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Rasgo { metodos, .. } => {
                assert!(matches!(
                    metodos[0].parametros[0].tipo,
                    Some(Tipo::Parametro(ref n)) if n == "T"
                ));
            }
            otra => panic!("se esperaba un rasgo, llegó: {otra:?}"),
        }
    }

    #[test]
    fn test_interpolacion_no_se_come_la_sentencia_siguiente() {
        // El lexer deja un fragmento de texto vacío al cerrar el string: ese
        // fragmento no debe enganchar el identificador de la línea siguiente.
        let prog = parsear_fuente("funcion f() {\n    a = \"v=${b}\"\n    a = 7\n}").unwrap();
        let cuerpo = match &prog.declaraciones[0] {
            Declaracion::Funcion { cuerpo, .. } => cuerpo,
            otra => panic!("se esperaba una función, llegó: {otra:?}"),
        };
        assert_eq!(cuerpo.len(), 2, "se fusionaron dos sentencias: {cuerpo:?}");
        match (&cuerpo[0], &cuerpo[1]) {
            (Declaracion::Asignacion { valor, .. }, Declaracion::Asignacion { valor: v2, .. }) => {
                assert!(matches!(**valor, Expresion::Binaria { .. }));
                assert!(matches!(**v2, Expresion::LiteralNumero(7)));
            }
            otra => panic!("se esperaban dos asignaciones, llegaron: {otra:?}"),
        }
    }

    #[test]
    fn parsea_si_dentro_de_argumentos() {
        // `si` como elemento de una lista de argumentos (layouts condicionales).
        // Antes el parser descartaba la llamada entera y dejaba las condiciones
        // sueltas como expresiones, sin avisar.
        let prog = parsear_fuente(
            "columna(etiqueta(\"a\"), si (x > 0) { boton(\"s\") } sino { texto(\"n\") })",
        )
        .unwrap();
        let (nombre, argumentos) = match &prog.declaraciones[0] {
            Declaracion::LlamadaFuncion { nombre, argumentos } => (nombre, argumentos),
            Declaracion::Expresion(Expresion::LlamadaFuncion { nombre, argumentos }) => {
                (nombre, argumentos)
            }
            otra => panic!("se esperaba la llamada a columna, llegó: {otra:?}"),
        };
        assert_eq!(nombre, "columna");
        assert_eq!(argumentos.len(), 2);
        match &argumentos[1] {
            Expresion::Si {
                bloque_verdadero,
                bloque_falso,
                ..
            } => {
                assert_eq!(bloque_verdadero.len(), 1);
                assert_eq!(bloque_falso.as_ref().map(|b| b.len()), Some(1));
            }
            otra => panic!("se esperaba Expresion::Si, llegó: {otra:?}"),
        }
    }

    #[test]
    fn parsea_si() {
        let prog = parsear_fuente("si (x > 0) { variable y = 1 } sino { variable z = 2 }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Si {
                bloque_verdadero,
                bloque_falso,
                ..
            } => {
                assert!(bloque_falso.is_some());
                assert_eq!(bloque_verdadero.len(), 1);
                assert_eq!(bloque_falso.as_ref().unwrap().len(), 1);
            }
            _ => panic!("Se esperaba Declaracion::Si"),
        }
    }

    #[test]
    fn parsea_mientras() {
        let prog = parsear_fuente("mientras (x < 10) { x = x + 1 }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Mientras { bloque, .. } => {
                assert_eq!(bloque.len(), 1);
            }
            _ => panic!("Se esperaba Declaracion::Mientras"),
        }
    }

    #[test]
    fn parsea_cuando() {
        let prog = parsear_fuente("cuando (x > 30) { x = 0 }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Cuando { cuerpo, .. } => {
                assert_eq!(cuerpo.len(), 1);
            }
            _ => panic!("Se esperaba Declaracion::Cuando"),
        }
    }

    #[test]
    fn parsea_para() {
        let fuente = "para (variable i = 0; i < 10; i = i + 1) { escribir(i) }";
        let prog = parsear_fuente(fuente).unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Para {
                inicializacion,
                condicion,
                incremento,
                bloque,
            } => {
                assert!(inicializacion.is_some());
                assert!(condicion.is_some());
                assert!(incremento.is_some());
                assert_eq!(bloque.len(), 1);
            }
            _ => panic!("Se esperaba Declaracion::Para"),
        }
    }

    #[test]
    fn parsea_repetir() {
        let prog = parsear_fuente("repetir (5) { escribir(\"hola\") }").unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Repetir { bloque, .. } => {
                assert_eq!(bloque.len(), 1);
            }
            _ => panic!("Se esperaba Declaracion::Repetir"),
        }
    }

    #[test]
    fn parsea_clase() {
        let fuente = "clase Persona { nombre constructor(n) { este.nombre = n } funcion saludar() { escribir(\"hola\") } }";
        let prog = parsear_fuente(fuente).unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Clase {
                nombre, metodos, ..
            } => {
                assert_eq!(nombre, "Persona");
                assert_eq!(metodos.len(), 2);
                assert_eq!(metodos[0].nombre, "nuevo"); // constructor -> nuevo
            }
            _ => panic!("Se esperaba Declaracion::Clase"),
        }
    }

    #[test]
    fn parsea_instanciacion() {
        let fuente = "variable alumno = nuevo Estudiante(\"Ana\")";
        let prog = parsear_fuente(fuente).unwrap();
        match &prog.declaraciones[0] {
            Declaracion::Variable { nombre, valor, .. } => {
                assert_eq!(nombre, "alumno");
                match valor.as_ref().unwrap() {
                    Expresion::Instanciacion { clase, argumentos } => {
                        assert_eq!(clase, "Estudiante");
                        assert_eq!(argumentos.len(), 1);
                    }
                    _ => panic!("Se esperaba Instanciacion"),
                }
            }
            _ => panic!("Se esperaba Declaracion::Variable"),
        }
    }

    #[test]
    fn parsea_escribir() {
        let fuente = "escribir(\"Hola mundo\")";
        let prog = parsear_fuente(fuente).unwrap();
        match &prog.declaraciones[0] {
            Declaracion::LlamadaFuncion { nombre, argumentos } => {
                assert_eq!(nombre, "escribir");
                assert_eq!(argumentos.len(), 1);
            }
            _ => panic!("Se esperaba LlamadaFuncion"),
        }
    }

    #[test]
    fn test_importar_sin_comillas() {
        let prog = parsear_fuente("importar std/io").unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::Importar(ruta) => assert_eq!(ruta, "std/io"),
            _ => panic!("Se esperaba Declaracion::Importar"),
        }
    }

    #[test]
    fn test_importar_con_comillas_relativa_es_alias() {
        // Alias retrocompatible: `importar "std/io"` == `importar std/io`.
        let prog = parsear_fuente("importar \"std/io\"").unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::Importar(ruta) => assert_eq!(ruta, "std/io"),
            _ => panic!("Se esperaba Declaracion::Importar"),
        }
    }

    #[test]
    fn test_importar_con_comillas_absoluta() {
        let prog = parsear_fuente("importar \"/home/user/lib.fa\"").unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::Importar(ruta) => assert_eq!(ruta, "/home/user/lib.fa"),
            _ => panic!("Se esperaba Declaracion::Importar"),
        }
    }

    #[test]
    fn test_importar_absoluta_windows() {
        let prog = parsear_fuente("importar \"C:/Users/user/lib.fa\"").unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::Importar(ruta) => assert_eq!(ruta, "C:/Users/user/lib.fa"),
            _ => panic!("Se esperaba Declaracion::Importar"),
        }
    }

    #[test]
    fn test_funcion_externa() {
        let prog = parsear_fuente("funcion externa printf(formato: Texto, ...: Texto) -> Entero;")
            .unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::Funcion {
                nombre,
                externa: true,
                ..
            } => {
                assert_eq!(nombre, "printf");
            }
            _ => panic!("Se esperaba Declaracion::Funcion con externa=true"),
        }
    }

    #[test]
    fn test_externo_funcion_vieja_sintaxis_error() {
        let resultado = parsear_fuente("externo funcion printf(formato: Texto) -> Entero;");
        assert!(
            resultado.is_err(),
            "sintaxis 'externo funcion' ya no es válida"
        );
    }

    #[test]
    fn test_funcion_externa_sin_punto_coma_error() {
        let resultado = parsear_fuente("funcion externa printf() {}");
        assert!(
            resultado.is_err(),
            "funcion externa debe terminar en ; no en llaves"
        );
    }

    #[test]
    fn test_importar_externa() {
        let prog = parsear_fuente("importar externa \"liberia.dll\"").unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::ImportarExterna(ruta) => assert_eq!(ruta, "liberia.dll"),
            _ => panic!("Se esperaba Declaracion::ImportarExterna"),
        }
    }

    #[test]
    fn test_importar_externa_variante() {
        let prog = parsear_fuente("importar externo \"liberia.so\"").unwrap();
        assert_eq!(prog.declaraciones.len(), 1);
        match &prog.declaraciones[0] {
            Declaracion::ImportarExterna(ruta) => assert_eq!(ruta, "liberia.so"),
            _ => panic!("Se esperaba Declaracion::ImportarExterna"),
        }
    }

    #[test]
    fn parsea_error_sintactico() {
        let fuente = "variable 123 = 5"; // 123 ahora es aceptado como nombre
        let resultado = parsear_fuente(fuente);
        assert!(resultado.is_ok()); // ahora acepta números como nombres de variable
    }

    #[test]
    fn parsea_comas_finales() {
        // Prueba las comas finales en llamadas, argumentos, tuplas, arreglos, mapas y parámetros calls, arguments, tuples, arrays, maps, and parameters.
        let sources = vec![
            "funcion f(a: Entero, b: Texto,) { retornar (a, b,) }",
            "variable x = f(1, \"hola\",)",
            "variable arr = [1, 2, 3,]",
            "variable mapa = { \"clave\": 1, }",
        ];
        for fuente in sources {
            let resultado = parsear_fuente(fuente);
            assert!(
                resultado.is_ok(),
                "Falló el parsing de trailing comma en: {}",
                fuente
            );
        }
    }

    /// una guarda `caso P si (cond)` se rechaza con un mensaje claro
    /// (Gramática) y ese error no se pierde por la recuperación del parser.
    #[test]
    fn test_guarda_en_patron_se_rechaza_con_mensaje_claro() {
        let fuente = "funcion a() { coincidir (1) { caso 1 si (2 > 0) -> { retornar 1 } } }";
        let errs = match parsear_fuente(fuente) {
            Ok(_) => panic!("la guarda debe diagnosticarse"),
            Err(errs) => errs,
        };
        assert!(
            format!("{}", errs[0]).contains("guardas en patrones"),
            "primer error inesperado: {}",
            errs[0]
        );
    }
}

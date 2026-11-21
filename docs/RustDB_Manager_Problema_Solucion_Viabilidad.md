# RustDB Manager
## Documento de problema, propuesta de solución y evaluación de viabilidad

**Tipo de documento:** Propuesta técnica para evaluación  
**Estado:** Borrador para revisión de un agente de desarrollo  
**Objetivo:** Determinar si es viable construir una herramienta en Rust para gestionar versiones, copias, diferencias y cambios de bases de datos por producto y cliente.

---

# 1. Resumen ejecutivo

Se propone desarrollar **RustDB Manager**, una herramienta que ayude a administrar la evolución de bases de datos de aplicaciones que se entregan y personalizan para distintos clientes.

El problema no está principalmente en el control de versiones del Backend o Frontend, porque esos componentes ya pueden gestionarse mediante Git. La dificultad está en la base de datos: su estructura, procedimientos almacenados, funciones, vistas, índices, restricciones, datos maestros y cambios particulares de cada cliente.

Cuando un cliente solicita una solución similar a la de otro, se reutiliza el proyecto y se realizan ajustes. Con el tiempo, las bases de datos de ambos clientes evolucionan de manera diferente. Cuando el primer cliente solicita una nueva mejora, puede ser difícil reconstruir el estado exacto de su base de datos. Se termina recurriendo a restauraciones manuales, copias antiguas o scripts de reversa que pueden fallar.

La propuesta es evaluar una herramienta que permita:

- Mantener una línea base versionada de cada producto.
- Crear copias o nuevas instancias de base de datos a partir de una versión conocida.
- Registrar los cambios aplicados por cliente y por ambiente.
- Separar cambios comunes de personalizaciones particulares.
- Comparar bases de datos y detectar diferencias no documentadas.
- Preparar paquetes de cambios para desplegar de forma controlada.
- Evitar que cambios de un cliente se filtren accidentalmente a otro.

**No se debe asumir que desarrollar una herramienta propia es necesariamente la mejor solución.** El agente debe comparar esta propuesta con herramientas existentes y recomendar si conviene construirla, integrar herramientas existentes o adoptar una solución híbrida.

---

# 2. Contexto actual

## 2.1 Componentes de una solución

Una solución típica está compuesta por:

- Backend.
- Frontend.
- Base de datos.
- Scripts SQL de creación y modificación.
- Procedimientos almacenados (SP).
- Funciones, vistas, triggers, índices y restricciones.
- Datos maestros o catálogos necesarios para el funcionamiento.

El código Backend y Frontend se puede versionar con Git. Sin embargo, la evolución de la base de datos no siempre está representada de forma completa, consistente y reproducible en el repositorio.

## 2.2 Escenario real

Ejemplo:

1. Se desarrolla una solución para el Cliente A.
2. El proyecto queda operativo y se entrega.
3. Aparece el Cliente B, que necesita prácticamente la misma solución, pero con algunos cambios.
4. Se reutiliza el proyecto y se modifican Backend, Frontend y base de datos para cubrir las necesidades del Cliente B.
5. Los cambios de la base de datos se ejecutan mediante scripts SQL y modificaciones manuales.
6. Tiempo después, el Cliente A solicita una mejora.
7. La base de datos que se tiene disponible puede contener cambios realizados para el Cliente B o no representar exactamente la versión que utiliza el Cliente A.
8. Se intenta restaurar una base anterior, conseguir una copia del cliente o ejecutar scripts `DOWN` para revertir cambios.
9. Las reversiones pueden fallar, dejar objetos incompatibles o perder información.
10. El Cliente A puede terminar recibiendo procedimientos almacenados, tablas, columnas o comportamientos que corresponden al Cliente B.

## 2.3 Problema principal

No existe un mecanismo confiable y centralizado que permita responder:

- ¿Qué versión de la estructura tiene cada cliente?
- ¿Qué cambios se aplicaron realmente en cada ambiente?
- ¿Qué procedimientos almacenados, tablas y otros objetos pertenecen al producto base?
- ¿Qué objetos son personalizaciones de un cliente?
- ¿Qué cambios están pendientes, aplicados o no corresponden a un cliente?
- ¿Desde qué versión conocida puedo crear una nueva copia?
- ¿Cómo puedo actualizar a un cliente sin introducir cambios de otro?
- ¿Cómo puedo detectar modificaciones manuales que no están documentadas?
- ¿Cómo puedo reconstruir una base sin depender de una copia física entregada por el cliente?

## 2.4 Consecuencias

- Riesgo de mezclar funcionalidades o estructuras entre clientes.
- Fallos en procedimientos almacenados por diferencias de estructura.
- Columnas, tablas o parámetros inexistentes en un ambiente.
- Pérdida de tiempo buscando copias antiguas.
- Dependencia de que el cliente entregue una copia actualizada.
- Reversiones `DOWN` incompletas o fallidas.
- Dificultad para reproducir errores.
- Despliegues con cambios que no corresponden al cliente.
- Falta de trazabilidad sobre qué se ejecutó, cuándo y dónde.

---

# 3. Alcance del problema que se quiere resolver

La herramienta debe centrarse inicialmente en la **gestión de la evolución de la base de datos**, no en reemplazar Git ni en administrar todo el ciclo de vida del Backend y Frontend.

Debe contemplar estos conceptos:

| Concepto | Descripción |
|---|---|
| Producto | Aplicación o solución que se entrega a distintos clientes. |
| Versión base | Versión común y reproducible de la estructura de base de datos. |
| Cliente | Organización que utiliza una instancia de la solución. |
| Personalización | Cambio que aplica únicamente a un cliente. |
| Cambio o migración | Unidad versionada de modificación de la base de datos. |
| Ambiente | DEV, UAT, PROD u otro ambiente. |
| Instancia | Base de datos física correspondiente a un cliente y ambiente. |
| Historial | Registro de migraciones efectivamente aplicadas. |
| Paquete de despliegue | Conjunto revisado de cambios que se pretende aplicar a un ambiente. |

## Fuera del alcance inicial

- Administrar o versionar datos transaccionales de los clientes.
- Reemplazar Git para el Backend y Frontend.
- Sincronizar automáticamente bases completas entre ambientes.
- Aplicar automáticamente cualquier diferencia detectada.
- Garantizar que un `DOWN` pueda revertir cualquier operación.
- Convertirse inicialmente en una plataforma multi-motor para SQL Server, MySQL y otros motores.
- Subir bases, credenciales o scripts empresariales a servicios personales o repositorios públicos.

---

# 4. Información y recursos disponibles

Para el caso inicial de reconstrucción de una línea base de producción, se confirmó la disponibilidad de:

| Recurso | Disponibilidad |
|---|---|
| Scripts completos de estructura | Sí |
| Scripts de datos maestros | Sí |
| Historial de controles desplegados en PROD | Sí |
| Acceso a una base local para pruebas | Sí |

El escenario inicial está orientado a **SQL Server**.

También existen scripts de cambios con operaciones `UP` y `DOWN`, por ejemplo:

- `018_fix_client_digiflow_urls_up.sql`
- `018_fix_client_digiflow_urls_down.sql`

Hay controles de cambio como `CC-001`, `CC-002`, etc. La numeración de scripts puede divergir entre ramas o controles, por lo que el número de archivo no debe ser la identidad principal de una migración.

**Advertencia:** aunque se disponga de estructura, datos maestros e historial, debe verificarse que los scripts reflejen fielmente lo que realmente se desplegó en producción. La existencia de un historial no demuestra por sí sola que no hubo modificaciones manuales posteriores.

---

# 5. Solución propuesta: RustDB Manager

Desarrollar una herramienta en Rust que gestione el ciclo de vida de las bases de datos por producto, cliente y ambiente.

La herramienta debe funcionar como una capa de control y trazabilidad. Git seguirá siendo el sistema de versionado de archivos y scripts.

## 5.1 Principios de diseño

1. **Una línea base conocida:** cada producto debe contar con una versión base identificable y reproducible.
2. **Historial por instancia:** cada base debe registrar sus propios cambios aplicados.
3. **Separación por cliente:** las personalizaciones deben estar asociadas explícitamente a un cliente.
4. **Migraciones inmutables:** no se deben editar scripts que ya fueron aplicados; las correcciones se realizan mediante nuevas migraciones.
5. **Comparación antes de ejecución:** detectar diferencias no significa aplicarlas automáticamente.
6. **Despliegues reproducibles:** promover el mismo paquete revisado entre ambientes.
7. **Trazabilidad:** guardar identificador, hash, fecha, resultado y ambiente de cada ejecución.
8. **Seguridad:** credenciales fuera del repositorio y privilegios mínimos.
9. **Datos protegidos:** no copiar datos transaccionales entre clientes salvo autorización y procedimiento específico.
10. **Transparencia:** diferenciar cambios explicados, diferencias desconocidas y cambios no aplicables.

## 5.2 Modelo conceptual

```text
PRODUCTO
  |
  +-- VERSION_BASE_1
  |      |
  |      +-- estructura común
  |      +-- procedimientos comunes
  |      +-- datos maestros comunes
  |
  +-- VERSION_BASE_2
         |
         +-- cambios comunes del producto

CLIENTE A
  |
  +-- versión base utilizada
  +-- personalizaciones A
  +-- historial por ambiente
  +-- instancias DEV / UAT / PROD

CLIENTE B
  |
  +-- versión base utilizada
  +-- personalizaciones B
  +-- historial por ambiente
  +-- instancias DEV / UAT / PROD
```

Una mejora común debe incorporarse a una nueva versión del producto base. Después debe evaluarse y aplicarse de forma controlada a cada cliente. Una personalización de un cliente no debe incorporarse automáticamente a los demás.

---

# 6. Funcionalidades propuestas

## 6.1 Database Manager: administración de bases

Debe permitir:

- Registrar conexiones a instancias de SQL Server.
- Listar bases disponibles.
- Inspeccionar metadatos de una base.
- Crear bases nuevas.
- Crear copias mediante backup/restore cuando sea apropiado.
- Reconstruir bases desde scripts.
- Verificar la integridad de una copia.
- Registrar origen, fecha, versión base y configuración de la copia.

La herramienta debe distinguir entre diferentes modalidades:

| Modalidad | Uso |
|---|---|
| Copia completa | Clonar una base, incluyendo sus datos, cuando esté autorizado. |
| Estructura + datos maestros | Crear una nueva instancia sin datos transaccionales. |
| Solo estructura | Crear una base vacía con objetos de base de datos. |
| Reconstrucción por migraciones | Construir una base desde una versión inicial y aplicar cambios registrados. |
| Backup/restore | Recuperar una instancia desde un respaldo físico válido. |

No debe asumirse que todas las modalidades son intercambiables. El método apropiado depende del tamaño, la seguridad, la consistencia y la disponibilidad de respaldos.

## 6.2 Baseline Manager: líneas base

Debe permitir:

- Registrar versiones base de cada producto.
- Reconstruir una base desde scripts conocidos.
- Registrar qué controles forman parte de la versión base.
- Calcular hashes de los archivos fuente.
- Generar un inventario de objetos.
- Validar la reconstrucción.
- Marcar una baseline como validada solo después de superar controles definidos.

Estados sugeridos:

- `draft`
- `building`
- `pending_validation`
- `validated`
- `validation_failed`
- `deprecated`

Una base reconstruida desde scripts debe identificarse como reconstruida. No debe presentarse como copia certificada de producción sin una comparación o validación suficiente.

## 6.3 Change Manager: controles y migraciones

Debe gestionar:

- Identificadores únicos de cambios.
- Archivos `UP` y, cuando corresponda, `DOWN`.
- Dependencias entre cambios.
- Objetos afectados.
- Orden de ejecución.
- Hash del contenido.
- Estado por ambiente.
- Evidencia de ejecución.

Ejemplo de manifiesto:

```json
{
  "migration_id": "20261121_CC002_001",
  "change_id": "CC-002",
  "product": "SISTEMA_ERP",
  "description": "Agregar configuración de integración",
  "engine": "SQLSERVER",
  "scope": "client",
  "client_id": "CLIENTE_B",
  "dependencies": [
    "20261120_BASE_003"
  ],
  "up": [
    "up/001_add_integration_config.sql"
  ],
  "down": [
    "down/001_add_integration_config.sql"
  ],
  "destructive": false
}
```

El esquema es ilustrativo. Debe evaluarse si conviene separar manifiestos de producto, cliente y migración.

El sistema no debe usar el número de archivo como identificador único. Debe mantener identificadores inmutables y detectar cambios en scripts que ya fueron aplicados.

## 6.4 Client Manager: administración de clientes

Debe permitir:

- Registrar clientes.
- Asociar un cliente a un producto y una versión base.
- Registrar personalizaciones exclusivas.
- Registrar las instancias y ambientes del cliente.
- Consultar qué cambios están aplicados y pendientes.
- Identificar cambios comunes y exclusivos.
- Crear una nueva instancia a partir de una plantilla permitida.

## 6.5 Database Comparator: comparación de bases

Debe comparar, inicialmente en SQL Server:

- Esquemas.
- Tablas.
- Columnas.
- Tipos, longitudes, precisión y escala.
- Nulabilidad y valores por defecto.
- Claves primarias y foráneas.
- Índices.
- Restricciones.
- Vistas.
- Procedimientos almacenados.
- Funciones.
- Triggers.
- Otros objetos relevantes que se definan para el motor.

Debe producir diferencias clasificadas:

- Objeto faltante.
- Objeto adicional.
- Definición modificada.
- Diferencia explicada por una migración registrada.
- Diferencia no explicada.
- Diferencia potencialmente peligrosa.

La comparación de datos maestros debe ser selectiva y basada en reglas explícitas. No debe comparar ni sincronizar indiscriminadamente datos transaccionales.

## 6.6 Deployment Manager: preparación de despliegues

En una etapa posterior debe:

- Seleccionar controles aprobados.
- Validar dependencias.
- Verificar que el cliente sea destinatario del cambio.
- Verificar la versión de origen de la base.
- Generar un plan de ejecución.
- Generar un paquete inmutable con scripts y hashes.
- Permitir revisión y aprobación.
- Ejecutar con confirmación explícita.
- Registrar resultados y errores.
- Validar el estado posterior a la ejecución.

No debe desplegar automáticamente todos los scripts de una carpeta ni aplicar todas las diferencias encontradas.

---

# 7. Escenario que la solución debe resolver

## 7.1 Situación inicial

Existe un producto base `ERP_V1`.

Se crea una instancia para dos clientes:

```text
ERP_V1
  |
  +-- CLIENTE_A_DB
  |
  +-- CLIENTE_B_DB
```

## 7.2 Cambios particulares

Cliente A recibe:

- `CC-A001`: agregar una columna.
- `CC-A002`: modificar un procedimiento almacenado.

Cliente B recibe:

- `CC-B001`: crear una tabla particular.
- `CC-B002`: modificar una regla de negocio.

El historial debe quedar separado:

| Cambio | Cliente A | Cliente B |
|---|---|---|
| ERP_V1 | Aplicado | Aplicado |
| CC-A001 | Aplicado | No aplica |
| CC-A002 | Aplicado | No aplica |
| CC-B001 | No aplica | Aplicado |
| CC-B002 | No aplica | Aplicado |

## 7.3 Cliente A solicita una nueva mejora

Cuando A solicita una nueva mejora, RustDB Manager debe:

1. Identificar la versión base de A.
2. Identificar las migraciones aplicadas a A.
3. Inspeccionar la base real de A.
4. Comparar el estado real con el estado esperado.
5. Identificar diferencias no registradas.
6. Preparar el nuevo cambio sobre la línea correcta.
7. No incluir migraciones exclusivas de B.
8. Permitir probar el paquete sobre una copia controlada de A.
9. Generar el paquete revisado para desplegarlo en los ambientes correspondientes.

El sistema debe advertir si la base real de A contiene objetos que no corresponden a su historial.

## 7.4 Cambio común para ambos clientes

Si se desarrolla una mejora que corresponde al producto base:

1. Crear una nueva versión base.
2. Identificar dependencias y compatibilidad.
3. Probar la actualización sobre una copia de A.
4. Probar la actualización sobre una copia de B.
5. Registrar el resultado por cliente.
6. Desplegar solo cuando cada instancia cumpla los requisitos.

No debe suponerse que una migración común se puede aplicar a todos los clientes sin adaptación.

---

# 8. Reconstrucción inicial de PROD_BASE

Se cuenta con scripts completos de estructura, datos maestros e historial completo de controles aplicados en PROD.

Se propone el siguiente procedimiento:

1. Validar que los archivos fuente estén completos y sin modificaciones inesperadas.
2. Crear una base vacía `PROD_BASE` en una instancia local de SQL Server.
3. Ejecutar scripts de creación de esquemas y objetos en orden de dependencias.
4. Cargar datos maestros mediante scripts autorizados.
5. Aplicar únicamente controles confirmados como desplegados en PROD, respetando su orden y dependencias.
6. Registrar cada ejecución y el hash de cada archivo.
7. Generar un inventario de objetos.
8. Validar tablas, columnas, tipos, claves, restricciones, índices y objetos programables.
9. Validar los datos maestros mediante conteos, claves y reglas de negocio definidas.
10. Comparar la reconstrucción con cualquier evidencia confiable disponible de PROD.
11. Generar un reporte de limitaciones y diferencias.
12. Marcar la baseline como validada solo si satisface los criterios de aceptación.

### Consideraciones críticas

- No aplicar indiscriminadamente todos los archivos `UP` disponibles.
- No confiar exclusivamente en el nombre o número de un script.
- No ejecutar scripts `DOWN` como método para reconstruir otra versión.
- Verificar dependencias entre controles.
- Algunos scripts DDL o DML pueden no ser transaccionales o reversibles.
- Los scripts de datos maestros deben distinguir registros administrados por el sistema de registros modificables por el negocio.
- Si no existe evidencia suficiente para confirmar que la reconstrucción representa PROD, registrar la baseline como reconstruida con limitaciones, no como copia exacta.

---

# 9. Arquitectura técnica candidata

La implementación inicial se orientaría a SQL Server.

| Componente | Alternativa candidata | Observaciones |
|---|---|---|
| Lenguaje | Rust | Lógica de la herramienta. |
| CLI | `clap` | Interfaz inicial por comandos. |
| Runtime asíncrono | `tokio` | Operaciones asíncronas. |
| Conexión SQL Server | `tiberius` u otra alternativa compatible | Validar mantenimiento y compatibilidad con las versiones objetivo. |
| Serialización | `serde` | Manifiestos y configuración. |
| Hashes | `sha2` | Integridad de scripts y paquetes. |
| Logs | `tracing` | Trazabilidad y diagnóstico. |
| Configuración | JSON/TOML | Conexiones y configuración no secreta. |
| Comparación SQL Server | DACPAC/SqlPackage o implementación propia | Evaluar antes de desarrollar un comparador DDL completo. |
| Migraciones | Motor propio acotado o integración con Flyway | Comparar costos, limitaciones y trazabilidad. |
| Interfaz futura | Tauri + React | Solo después de validar el núcleo CLI. |

**No se debe seleccionar una biblioteca solo por popularidad.** El agente debe verificar compatibilidad real, mantenimiento, licencias, seguridad, soporte para las versiones de SQL Server y capacidades necesarias.

## 9.1 Posible estructura del repositorio

```text
rustdb-manager/
├── products/
│   └── sistema-erp/
│       ├── baselines/
│       │   └── v1/
│       │       ├── schema/
│       │       ├── master-data/
│       │       └── manifest.json
│       └── migrations/
│
├── clients/
│   ├── cliente-a/
│   │   ├── manifest.json
│   │   └── migrations/
│   └── cliente-b/
│       ├── manifest.json
│       └── migrations/
│
├── environments/
│   └── connections.example.toml
│
├── packages/
├── reports/
├── docs/
├── src/
│   ├── database/
│   ├── backup_restore/
│   ├── cloning/
│   ├── baseline/
│   ├── migrations/
│   ├── clients/
│   ├── comparator/
│   ├── deployment/
│   └── validation/
│
├── tests/
├── Cargo.toml
└── README.md
```

Esta estructura es una hipótesis inicial. El agente puede proponer otra si mejora el aislamiento, la mantenibilidad o el modelo de versionado.

---

# 10. Estrategias de copia y aislamiento

La herramienta debe evaluar y soportar, según el caso, diferentes mecanismos:

## A. Backup y Restore de SQL Server

Ventajas:
- Copia fiel de una base existente.
- Conserva estructura y datos.
- Útil para pruebas sobre una instancia real del cliente, si existe autorización.

Limitaciones:
- Requiere un backup válido y acceso.
- Puede ser pesado.
- Debe tratarse con cuidado la información sensible.
- Restaurar una base no la convierte automáticamente en una nueva versión controlada.

## B. Reconstrucción desde baseline y migraciones

Ventajas:
- Reproducible.
- No necesita copiar datos transaccionales.
- Facilita crear nuevas instancias limpias.
- Permite auditar qué scripts se aplicaron.

Limitaciones:
- Requiere scripts completos, ordenados y probados.
- Puede no reconstruir datos que no estén documentados.
- Los cambios manuales no registrados no aparecerán por arte de magia.

## C. Clonación de una base existente

Ventajas:
- Puede ser práctica para preparar ambientes de prueba de un cliente.

Limitaciones:
- Puede copiar cambios no documentados.
- Puede copiar datos sensibles.
- Debe registrar origen, fecha, versión y propósito.
- Debe evitar que se confunda una copia física con una baseline limpia.

El agente debe recomendar cuándo usar cada estrategia y qué controles implementar.

---

# 11. Riesgos y controles obligatorios

| Riesgo | Control propuesto |
|---|---|
| Cambios de un cliente aplicados a otro | Validación de destinatario y alcance de cada migración. |
| Modificación de un script ya aplicado | Hash y bloqueo de migraciones inmutables. |
| Diferencias manuales en una base | Comparación con historial y reporte de drift. |
| Reversa fallida | No depender de `DOWN` como mecanismo universal; respaldos y migraciones correctivas. |
| Pérdida de datos | Backup, validación, aprobación y pruebas previas. |
| Dependencias omitidas | Grafo de dependencias y validación antes de despliegue. |
| Copia de datos sensibles | Políticas de acceso, enmascaramiento y separación por cliente. |
| Ejecución en base equivocada | Confirmación del servidor, base, cliente y ambiente. |
| Error parcial durante despliegue | Registro por paso y estrategia de recuperación. |
| Cambios destructivos | Aprobación explícita y plan de recuperación. |
| Credenciales expuestas | Secretos fuera de Git, permisos mínimos y rotación. |
| Baseline incorrecta | Evidencia de procedencia y estado de validación. |

El sistema debe iniciar en modo de solo lectura para inspección y comparación. Las operaciones que escriben o modifican una base deben requerir una acción explícita.

---

# 12. MVP propuesto

No se recomienda implementar todos los módulos al mismo tiempo.

## MVP 0.1 — Reconstrucción y comparación de SQL Server

### Objetivo

Demostrar que la herramienta puede reconstruir una base desde scripts controlados y comparar su estructura con otra base, identificando diferencias.

### Funcionalidades

1. Conectarse a SQL Server.
2. Inspeccionar una base.
3. Obtener inventario de tablas, columnas, claves, índices, vistas, procedimientos, funciones y triggers.
4. Validar archivos fuente y hashes.
5. Construir una base local desde scripts de estructura.
6. Cargar datos maestros de prueba.
7. Registrar qué scripts se aplicaron.
8. Comparar `DEMO_PROD_BASE` contra `DEMO_DEV`.
9. Generar reporte Markdown y JSON.
10. Detectar objetos no explicados por el historial.

### Comandos candidatos

```bash
rustdb connection test --profile local

rustdb schema inspect --database DEMO_PROD_BASE

rustdb baseline validate --product sistema-erp --version v1

rustdb baseline build --product sistema-erp --version v1 \
  --database DEMO_PROD_BASE

rustdb compare --source DEMO_PROD_BASE --target DEMO_DEV

rustdb report --format markdown
```

Los comandos son propuestas; deben revisarse antes de implementarse.

### Fuera del MVP 0.1

- Despliegue automático a producción.
- Reversión automática.
- Sincronización de datos.
- Soporte para MySQL.
- Interfaz gráfica.
- Administración avanzada de clientes.
- Generación automática de migraciones a partir de cualquier diferencia.

## MVP 0.2 — Versionado por cliente

- Registrar productos y clientes.
- Asociar una versión base a cada cliente.
- Registrar migraciones por cliente.
- Registrar historial por ambiente.
- Validar dependencias y alcance.
- Detectar cambios que no corresponden a un cliente.

## MVP 0.3 — Copias y restauración

- Crear una base desde una baseline.
- Crear una copia mediante backup/restore.
- Permitir seleccionar estructura, datos maestros o copia completa.
- Registrar origen y versión.
- Verificar la copia.
- Aplicar políticas de protección de datos.

## MVP 0.4 — Paquetes de despliegue

- Seleccionar cambios.
- Validar dependencias.
- Crear paquete inmutable.
- Generar plan y script de ejecución.
- Exigir aprobación.
- Registrar resultado.
- Validar el estado posterior.

---

# 13. Criterios de aceptación del MVP 0.1

El MVP se considerará viable técnicamente si demuestra, al menos, lo siguiente:

1. Puede conectarse a una instancia local de SQL Server.
2. Puede obtener un inventario consistente de los objetos definidos para la primera versión.
3. Puede construir una base vacía desde un conjunto de scripts de demostración.
4. Puede cargar datos maestros de demostración.
5. Puede registrar los scripts ejecutados y sus hashes.
6. Puede detectar tablas, columnas y procedimientos faltantes o adicionales en otra base.
7. Puede identificar modificaciones en definiciones de objetos soportados.
8. Puede generar un reporte reproducible.
9. No modifica la base comparada durante una operación de inspección.
10. Puede distinguir diferencias conocidas de diferencias no explicadas, cuando se haya proporcionado un historial confiable.

Se deben definir pruebas con dos bases de datos de demostración y cambios deliberados.

---

# 14. Preguntas que el agente debe investigar y resolver

El agente debe realizar una evaluación crítica, apoyada en documentación técnica actualizada y pruebas cuando sea necesario.

## A. Viabilidad general

1. ¿Es técnicamente viable implementar RustDB Manager en Rust?
2. ¿Qué partes conviene desarrollar en Rust y cuáles delegar a herramientas existentes?
3. ¿La solución aporta valor real frente a Flyway, Liquibase, DbUp, SQL Server DACPAC/SqlPackage o SQL Database Projects?
4. ¿Conviene una solución propia, una integración o un enfoque híbrido?
5. ¿Qué complejidad y mantenimiento tendría?

## B. Versionado de bases de datos

6. ¿Cómo representar correctamente una versión base y las migraciones posteriores?
7. ¿Cómo separar migraciones comunes del producto y migraciones exclusivas del cliente?
8. ¿Cómo registrar el estado por cliente y por ambiente?
9. ¿Cómo gestionar migraciones fuera de orden, dependencias y ramas divergentes?
10. ¿Cómo detectar que un script ya aplicado fue modificado?
11. ¿Cómo manejar cambios manuales realizados directamente en una base?

## C. Copias y reconstrucción

12. ¿Qué mecanismo es más adecuado para crear copias en SQL Server: backup/restore, reconstrucción desde scripts o ambos?
13. ¿Cómo crear una instancia nueva con estructura y datos maestros, pero sin datos transaccionales?
14. ¿Cómo asegurar que los datos maestros no sobrescriban modificaciones legítimas del cliente?
15. ¿Cómo verificar que una reconstrucción representa realmente una versión esperada?
16. ¿Cómo registrar la procedencia y el nivel de confianza de cada copia?

## D. Comparación de estructuras

17. ¿Qué herramienta permite comparar de forma confiable objetos de SQL Server?
18. ¿Puede usarse SqlPackage/DACPAC desde una aplicación Rust?
19. ¿Qué objetos y diferencias no detectaría un comparador básico?
20. ¿Cómo detectar diferencias en definiciones de SP, funciones, vistas y triggers?
21. ¿Cómo distinguir diferencias esperadas por una personalización de diferencias accidentales?

## E. Reversión y recuperación

22. ¿Es correcto mantener scripts `DOWN` para todas las migraciones?
23. ¿Qué operaciones DDL/DML de SQL Server no son fácilmente reversibles?
24. ¿Cómo tratar migraciones destructivas?
25. ¿Qué estrategia de backup, recuperación y validación debe aplicarse antes de un despliegue?
26. ¿Cómo manejar una ejecución parcial en la que algunos scripts terminaron y otros fallaron?

## F. Seguridad

27. ¿Qué permisos mínimos necesita la herramienta para inspeccionar, reconstruir, copiar y desplegar?
28. ¿Cómo almacenar y proteger credenciales?
29. ¿Cómo evitar la exposición de datos transaccionales y personales al clonar bases?
30. ¿Qué auditoría debe guardarse para cada operación?

## G. Diseño y roadmap

31. ¿Cuál es el modelo de datos mínimo para productos, clientes, versiones, migraciones, ambientes e instancias?
32. ¿Qué estructura de repositorio recomendarías?
33. ¿Cuál debería ser el MVP más pequeño que demuestre valor?
34. ¿Qué pruebas automatizadas son imprescindibles?
35. ¿Qué funcionalidades deberían descartarse o postergarse?
36. ¿Qué riesgos podrían hacer que el proyecto no sea conveniente?

---

# 15. Entregables solicitados al agente

No empezar a desarrollar la aplicación todavía. Primero entregar una evaluación.

El análisis debe incluir:

1. **Diagnóstico del problema:** confirmar si la solución propuesta ataca las causas reales.
2. **Evaluación de viabilidad:** viable, viable con restricciones o no recomendable.
3. **Comparación de alternativas:** desarrollo propio, herramientas existentes e integración híbrida.
4. **Arquitectura recomendada:** componentes, responsabilidades y flujo.
5. **Modelo de versionado:** producto base, cliente, migración, ambiente e instancia.
6. **Estrategia de copias:** backup/restore frente a reconstrucción.
7. **Estrategia de comparación:** herramientas y limitaciones.
8. **Riesgos:** técnicos, operativos y de seguridad.
9. **MVP recomendado:** alcance, tareas, pruebas y criterios de aceptación.
10. **Estimación:** complejidad por módulo, dependencias y principales incertidumbres.
11. **Decisiones pendientes:** preguntas que requieren información adicional.
12. **Recomendación final:** construir, integrar herramientas existentes, adoptar una solución híbrida o descartar la idea.

Para cada recomendación importante, explicar sus ventajas, desventajas y razones. No inventar capacidades de librerías ni asumir compatibilidad sin verificarla.

---

# 16. Instrucciones para el agente

Actúa como arquitecto de software y especialista en administración de bases de datos, SQL Server, migraciones, versionado y desarrollo con Rust.

Evalúa esta propuesta con criterio crítico.

**No debes validar la idea por compromiso.** Si existe una solución madura que resuelva mejor una parte del problema, recomiéndala. Si desarrollar un componente propio añade complejidad innecesaria, indícalo.

Prioriza:

- Evitar contaminación entre clientes.
- Poder reconstruir una base de forma reproducible.
- Mantener historial real por cliente y ambiente.
- Detectar diferencias no documentadas.
- Crear copias seguras y verificables.
- Evitar pérdida de datos y despliegues incorrectos.
- Reducir la dependencia de restauraciones manuales y reversiones frágiles.

Distingue claramente entre:

- Copiar una base existente.
- Reconstruir una base desde scripts.
- Crear una base desde una versión base y sus migraciones.
- Comparar dos bases.
- Sincronizar o desplegar cambios.

Son operaciones diferentes y no deben confundirse.

**Primera fase de tu trabajo:** analizar la propuesta, investigar las alternativas y emitir un dictamen de viabilidad. No implementar todavía el proyecto ni modificar bases de datos reales.

---

# 17. Resultado esperado

Al finalizar la evaluación se debe poder responder:

> ¿Podemos construir una herramienta que permita crear y mantener bases independientes por cliente, conocer exactamente qué cambios tiene cada una, reconstruirlas desde una versión controlada y evitar que las personalizaciones de un cliente afecten a los demás?

Y, sobre todo:

> ¿Conviene construir RustDB Manager en Rust o es mejor combinar herramientas existentes para resolver el problema con menor riesgo y mantenimiento?

Ese es el criterio principal para decidir si se inicia el desarrollo.

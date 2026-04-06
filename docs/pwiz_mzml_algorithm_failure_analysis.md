# Analisis de Fallos de Algoritmo (PWiz mzML Migration)

Fecha de corte: 2026-02-12  
Suite: `crates/parser/tests/pwiz_mzml.rs`  
Resultado actual: `63` tests, `52` pass, `11` fail.

## Resumen ejecutivo

Los `11` fallos actuales se explican por `2` defectos de algoritmo en el parser/serializer:

1. Perdida de `ms_level` en round-trip `B000` (9 tests).
2. Perdida de `binaryDataArray` vacios al serializar `mzML` (2 tests).

No se detectan fallos de performance en esta etapa; son fallos de correctness de formato.

## Mapa de fallos por grupo

### Grupo A: `ms_level` perdido (`9` tests)

Tests afectados:

- `pwiz_binary_data_encoder_roundtrip_across_levels`
- `pwiz_msdatafile_mzml_subset_parse_encode_decode_tiny10`
- `pwiz_msdatafile_mzml_subset_parse_encode_decode_tiny11`
- `pwiz_msdatafile_mzml_subset_parse_bin_to_mzml_parse_then_b000_roundtrip`
- `pwiz_msdatafile_mzml_subset_repeated_roundtrip_stability`
- `pwiz_parse_and_roundtrip_parser_internal_fixture_regression_guard`
- `pwiz_parse_and_roundtrip_parser_internal_fixture_anpc_regression_guard`
- `pwiz_serializer_mzml_b000_roundtrip_tiny_10_level0`
- `pwiz_serializer_mzml_b000_roundtrip_tiny_11_level12`

Fallo observado:

- Assert en `crates/parser/tests/pwiz_mzml.rs:216`:
  `left: Some(1) right: None` para `Spectrum.ms_level`.

### Grupo B: arrays binarios vacios perdidos (`2` tests)

Tests afectados:

- `pwiz_serializer_mzml_tiny_10_roundtrip_semantic`
- `pwiz_serializer_mzml_tiny_11_roundtrip_semantic`

Fallo observado:

- Assert en `crates/parser/tests/pwiz_mzml.rs:236`:
  `binaryDataArray count mismatch left: 2 right: 0` (caso `scan=21`, `defaultArrayLength=0`).

## Causa raiz y puntos exactos

## 1) Perdida de `ms_level` en decode `B000`

### Evidencia tecnica

- En decode de B000, el parseo de spectrum lee `ms_level` solo como atributo:
  `crates/parser/src/b64/utilities/parse_spectrum_list.rs:206`.
- Los `cv_params` del spectrum si se parsean:
  `crates/parser/src/b64/utilities/parse_spectrum_list.rs:215`.
- Pero no existe fallback desde cv term `MS:1000511` para poblar `Spectrum.ms_level`.

En el parser XML normal ya existe este fallback:

- `maybe_set_ms_level(...)` en `crates/parser/src/mzml/parse_mzml.rs:210`.
- Uso dentro de parseo de spectrum XML en `crates/parser/src/mzml/parse_mzml.rs:1533` y `crates/parser/src/mzml/parse_mzml.rs:1548`.

### Diagnostico

En B000, `ms_level` viene como `cvParam` y no necesariamente como atributo B000 dedicado.  
El objeto `Spectrum` queda con `ms_level=None` aunque `cv_params` contiene `MS:1000511`.

### Correccion sugerida (recomendada)

Agregar fallback equivalente al parser XML dentro de parseo B000:

1. En `crates/parser/src/b64/utilities/parse_spectrum_list.rs`, despues de:
   `let (cv_params, user_params) = parse_cv_and_user_params(...)`
2. Si `ms_level` es `None`, buscar cv accession `MS:1000511` en `cv_params`.
3. Parsear `value` a `u32` y asignar `Spectrum.ms_level`.

### Alternativa (menos recomendada)

Emitir siempre `ACC_ATTR_MS_LEVEL` como atributo B000 en encode.  
Es mas invasivo porque cambia el contrato de serializacion y tests de utilidades B000.

## 2) Perdida de `binaryDataArray` con payload vacio en `bin_to_mzml`

### Evidencia tecnica

- En writer de arrays, si `bda.binary` es `None`, se retorna temprano:
  `crates/parser/src/mzml/bin_to_mzml.rs:1370`.
- Esto omite la escritura del nodo `<binaryDataArray>` completo.
- Sin embargo el `binaryDataArrayList` se abre con `count` original:
  `crates/parser/src/mzml/bin_to_mzml.rs:1327`.

En fixtures PWiz existen spectra con arrays vacios validos:

- `scan=21` en `pwiz/example_data/tiny.pwiz.1.1.mzML:208`.
- Tiene `binaryDataArrayList count="2"` con `encodedLength="0"`:
  `pwiz/example_data/tiny.pwiz.1.1.mzML:218`.

### Diagnostico

El parser XML deja `binary=None` cuando `<binary>` esta vacio (comportamiento aceptable),  
pero el serializer interpreta `None` como "no escribir array", perdiendo estructura.

### Correccion sugerida (recomendada)

En `write_binary_data_array(...)`:

1. No hacer `return Ok(())` cuando `binary` es `None`.
2. Escribir igualmente `<binaryDataArray>` preservando:
   - `arrayLength` (si existe),
   - `encodedLength=0`,
   - `cvParam`/`userParam`/`referenceableParamGroupRef`,
   - `<binary></binary>` vacio.
3. Tratar `binary=None` como payload vacio estructural, no como "drop entry".

### Alternativa

En parse XML, mapear `<binary>` vacio a `BinaryData::*` vacio (en vez de `None`),  
pero sigue siendo recomendable robustecer writer para aceptar `None` sin eliminar nodos.

## Riesgos adicionales detectados (no bloquean estos 11)

Revisando utilidades B000 se observan campos que se inicializan vacios en parse:

- `Spectrum.referenceable_param_group_refs`:
  `crates/parser/src/b64/utilities/parse_spectrum_list.rs:242`.
- `Scan.referenceable_param_group_refs`:
  `crates/parser/src/b64/utilities/parse_scan_list.rs:86`.
- `BinaryDataArray.referenceable_param_group_refs`:
  `crates/parser/src/b64/utilities/parse_binary_array_list.rs:99`.

El encode si serializa estos refs:

- Scans: `crates/parser/src/b64/encode.rs:589`.
- Spectra: `crates/parser/src/b64/encode.rs:784`.
- Binary arrays: `crates/parser/src/b64/encode.rs:886`.

Sugerencia: agregar tests dedicados de round-trip para refs en esos tres niveles.

## Orden de correccion sugerido

1. Corregir fallback `ms_level` en decode B000.
2. Corregir escritura de `binaryDataArray` vacios en `bin_to_mzml`.
3. Re-ejecutar `cargo nextest run -p octo --test pwiz_mzml --no-fail-fast`.
4. Si todo queda verde, reforzar con tests de refs B000 (riesgo adicional).

## Criterio de cierre

Este bloque se considera cerrado cuando:

- Los `11` tests anteriores pasan.
- No se introducen regresiones en la suite `b64` existente.
- `pwiz_mzml` queda `63/63` en `cargo nextest`.


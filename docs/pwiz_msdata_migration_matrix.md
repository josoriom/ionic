# Matriz de Migracion PWiz -> Octo (mzML)

Estado actual de migracion real en `crates/parser/tests/pwiz_mzml.rs`.

## Resumen

- Total tests migrados/adaptados + adversariales: `68`
- Resultado actual: `52` passing / `16` failing
- Objetivo: correctness de `mzML` (no performance-first)

## Cobertura por suite PWiz

| PWiz suite | Cobertura en Octo | Estado |
|---|---|---|
| `Serializer_mzML_Test` | Round-trip semantico/estructural, matrix de config `B000`, round-trip via `bin_to_mzml` | Migrada |
| `SpectrumList_mzML_Test` | identidad, `find` por id, `findNameValue`, `findSpotID`, payload binario punto a punto, precursor refs | Migrada |
| `ChromatogramList_mzML_Test` | identidad, tamanos, payload binario punto a punto (`tic`, `sic`) | Migrada |
| `MSDataFileTest` (subset mzML) | parse/encode/decode, flujo `mzML -> xml -> mzML`, estabilidad de round-trip | Migrada (subset) |
| `ReaderTest` (subset mzML) | parse de fixtures mzML, comportamiento slim/full | Migrada (subset) |
| `SpectrumListCacheTest` (subset mzML) | acceso repetido estable spectrum/chromatogram | Migrada (subset) |
| `BinaryDataEncoderTest` | firma/header, determinismo, niveles de compresion, corrupcion/truncamiento, preservacion de IDs y arrays enteros | Migrada |
| `IOTest` (subset mzML) | parse de namespaces, estabilidad parse->write->parse, external metadata por `referenceableParamGroupRef`, slim semantics | Migrada (subset) |
| `DiffTest` (equivalente Rust) | fingerprint semantico estable, mutacion critica, comparacion binary-only ignorando identidad | Migrada (adaptada) |
| `MSDataTest` (equivalente Rust) | invariantes de `defaultArrayLength` vs payload | Migrada (adaptada) |
| `ReferencesTest` | refs internas resueltas + negativos con refs rotas | Migrada |
| `SpectrumInfoTest` | valores conocidos scan 19/20, RT/mzLow/mzHigh, precursor m/z-intensity-charge | Migrada |

## Casos PWiz no equivalentes 1:1 (por API)

| Caso PWiz | Motivo de adaptacion |
|---|---|
| `Serializer_mzML_Test::testWriteSkipError` | En Octo no existe API de escritura con callback de error por spectrum para "skip/continue". Se cubre con negativos de corrupcion decode. |
| `ReaderTest::identifyAsReader` multi-formato | Octo expone parser `mzML` directo (no `ReaderList` polimorfico multi-formato). Se migro ruta `mzML` pura. |
| `IOTest` callbacks/progress/positions | Octo no expone writer XML con hooks de progreso/offset iguales a PWiz. Se migro correctness semantico del contenido. |
| `DiffTest` engine interno PWiz | Octo no tiene `Diff<>` equivalente tipado. Se migro con comparadores semanticos/fingerprints y validaciones de payload. |

## Falla actuales (gaps de parser/serializer detectados)

1. `ms_level` se pierde tras `encode/decode` `B000` en varios caminos de round-trip.
2. `bin_to_mzml` pierde arrays binarios en spectra con `defaultArrayLength=0` (mismatch de shape).

Los fallos son intencionales como señal de correctness no cumplida; no se modifico el parser.
Ademas de los 2 gaps iniciales, ahora hay pruebas adversariales nuevas para perdida de metadata B000.

# Ejecutar Suite mzML (nextest)

## Preparar fixtures PWiz minimos

```bash
./scripts/fetch_pwiz_subset.sh
```

Esto descarga solo:

- `pwiz/data/msdata`
- `example_data`

con `sparse-checkout` + `--filter=blob:none`.

## Ejecutar toda la suite con nextest

```bash
./scripts/run-mzml-tests.sh
```

Actualmente la suite `pwiz_mzml` contiene `63` tests (PWiz mzML migration).

## Ejecutar solo la suite nueva (integration test)

```bash
cargo nextest run -p octo --test pwiz_mzml
```

## Compilar la suite sin ejecutarla

```bash
cargo test -p octo --test pwiz_mzml --no-run
```

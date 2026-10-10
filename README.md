# Savia — Plataforma de donaciones on-chain (Soroban/Stellar)

Smart contract en Rust para una plataforma de donaciones: campañas con objetivo y plazo, donaciones con fee de plataforma, trust score por donante, badges NFT por donación, y desembolsos por hitos.

**Aclaración**: el repo se llama `soroban-hello-world` pero el contrato es `savia` — never juzgues un repo por su nombre.

## Estado del revival (2026-10)

El contrato se abandonó con **tests que nunca habían corrido** y **build a WASM roto**. Ahora:

- **✅ 4/4 tests pasan** (`cargo test`) — antes no compilaban
- **✅ Build a WASM** (`cargo build --target wasm32-unknown-unknown --release`) → `savia.wasm` (~24KB)
- **🐛 Bugs de build arreglados**:
  - Faltaba `features = ["alloc"]` en soroban-sdk → sin global allocator, el WASM no linkeaba
  - `Option<BytesN<32>>` en `NFTBadge` no es serializable como `#[contracttype]` → campo plano
  - `.to_string()` sobre `soroban_sdk::String` no existe en `no_std` → helper `string_to_bytes()` vía `copy_into_slice`
  - `ethnum 1.5.2` (dep transitiva) no compila con toolchains nuevos → bump a 1.5.3
- **🔧 Mejora de API**: `donate` ahora devuelve `(donation_id, Option<nft_id>)` — antes minteaba el badge y su ID quedaba **irrecuperable** desde afuera
- **🧪 Tests que verifican comportamiento real**: fee 2% aplicado (1000 → 980), `current_amount` de campaña, badge referencia a campaña, rechazo de goal=0

## Cómo correr

```bash
rustup target add wasm32-unknown-unknown
cargo test                                        # 4 tests
cargo build --target wasm32-unknown-unknown --release
```

## Arquitectura

| Entidad | Campos clave |
|---|---|
| `Campaign` | id, title, description, beneficiary, goal/current amount, start/end time, verified, trust_score, category, location |
| `Donation` | id, campaign_id, donor, amount (neto), timestamp, nft_minted, anonymous |
| `TrustScore` | entity, score (0-100), verification_level, counts, total_donated, last_updated |
| `NFTBadge` | id, owner, badge_type, campaign_id, minted_at, metadata_uri |
| `Disbursement` | id, campaign_id, recipient, amount, milestone, status, timestamps |

| Función | Qué hace |
|---|---|
| `initialize(fee)` | fee de plataforma en basis points ≤ 1000 (10%) |
| `create_campaign(...)` | valida goal > 0 y duración 1-365 días; ID = sha256(inputs) |
| `verify_campaign(id, score)` | marca verificada (⚠️ ver deuda técnica) |
| `donate(...)` | valida campaña activa y monto > 0; descuenta fee; actualiza trust score; mintea badge |
| `create_disbursement(...)` | valida fondos disponibles; milestone como string |
| `execute_disbursement(id)` | solo si está `Approved` |

## ⚠️ Deuda técnica documentada (NO tocada en el revival)

1. **Sin autenticación**: ninguna función llama `env.require_auth()` — cualquiera puede verificar campañas o donar en nombre de otro. Para producción hay que agregar auth de admin/donante.
2. **Disbursements no descuentan**: `execute_disbursement` cambia el status pero no resta `current_amount` de la campaña — los fondos se pueden "ejecutar" más de lo recaudado.
3. **Trust score con aritmética frágil**: `(20 * amount_factor / 100000) * consistency / 100` sobre u64 con `overflow-checks = true` en release — puede entrar en pánico controlado con montos enormes.
4. **`platform_fee_rate` default 200 (2%) si no se inicializó** — el estado "no inicializado" es indefinido.

## Tests

```bash
cargo test
```

- `test_initialize_contract` — init sin pánico, idempotente
- `test_create_campaign` — campos persistidos correctamente
- `test_donation_applies_platform_fee` — **fee 2%: donación de 1000 → 980 neto** a la campaña + badge NFT referencia la campaña
- `test_create_campaign_rejects_invalid_goal` — goal=0 falla

## Licencia

El workspace no declara licencia.

## Autor

[FernandoMay](https://github.com/FernandoMay)

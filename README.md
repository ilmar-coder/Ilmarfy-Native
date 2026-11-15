# Ilmarfy Native

Apps nativos do **Ilmarfy** (o player de música da Private Cloud) usando o mesmo servidor,
biblioteca e playlists do Ilmarfy web — sem alterar o web.

| Parte | Situação |
|---|---|
| `crates/ilmarfy-core` — núcleo em Rust compartilhado | ✅ fase 1 |
| `crates/ilmarfy-cli` — linha de comando para testar o núcleo | ✅ |
| App Android (Kotlin + Jetpack Compose + Media3, núcleo via UniFFI) | próxima fase |
| App de desktop para Mac e Fedora (Rust + egui; RPM gerado pelo GitHub Actions) | depois do Android |

## O que o núcleo faz

- **Login por aparelho**: a senha é trocada uma vez por um token (`Authorization: Bearer ilf_…`) revogável;
  a senha nunca é guardada.
- **Cache local em SQLite**: biblioteca, playlists, artistas, álbuns, mais tocadas, busca sem acentos — funciona offline.
- **Sincronização com ETag**: só baixa a biblioteca quando ela muda no servidor.
- **Offline**: baixa faixas/playlists para o aparelho; `source()` escolhe arquivo local ou streaming.
- **Plays offline**: ficam guardados e vão ao servidor em lote, sem contar duas vezes.
- **Fila**: ordem, aleatório, repetir tudo/uma, tocar a seguir, adicionar ao fim.
- **Flow**: sugestões, 👎 música, 🚫 artista e Restritos.

Servidor padrão: `https://download.ilmar.com.br` (API documentada pelo código em `crates/ilmarfy-core/src/api.rs`).

## Usando o CLI

```bash
cargo run -p ilmarfy-cli -- login        # pede a senha sem mostrar; guarda só o token
cargo run -p ilmarfy-cli -- sync
cargo run -p ilmarfy-cli -- stats
cargo run -p ilmarfy-cli -- search sepultura roots
cargo run -p ilmarfy-cli -- help
```

## Testes

```bash
cargo test --workspace                               # rápidos, sem rede
cargo test -p ilmarfy-core -- --ignored --test-threads 1   # contra o servidor real (precisa do login do CLI)
```

# CryptoMiner

> [Read this documentation in English](README.md)

O CryptoMiner é um projeto pessoal de estudo para aprofundar o uso de **Rust** e explorar como **machine learning** pode apoiar atividades cotidianas orientadas por dados.

Ele funciona como um laboratório prático de programação de sistemas, concorrência, integração com APIs, observabilidade e interfaces de terminal. Experimentos com machine learning — como análise de métricas, identificação de padrões e automação baseada em dados — fazem parte da direção de evolução do projeto, mas **ainda não são funcionalidades implementadas**.

## Status e escopo

Este repositório tem finalidade de aprendizado e experimentação. Use os recursos de mineração somente de forma local e responsável. Antes de executar qualquer carga de trabalho, considere o gasto de energia, o impacto no hardware e as exigências legais aplicáveis. Nunca execute software de mineração em equipamentos que não sejam seus ou para os quais você não tenha autorização explícita.

## Visão geral

Gerenciador de mineração solo para múltiplas moedas, escrito em Rust. Ele pode minerar Monero (XMR) na CPU com RandomX ou orquestrar mineradores de GPU externos, como lolMiner, TeamRedMiner e XMRig. Também inclui um servidor Stratum V1 integrado para que mineradores externos se conectem a um nó local.

## Recursos

- **Mineração em CPU** — hashing RandomX via FFI para mineração solo de Monero.
- **Mineração em GPU** — gerenciamento de processos externos, com reinicialização automática.
- **Servidor Stratum V1** — proxy integrado para mineradores externos.
- **RPC de daemon Monero** — cliente JSON-RPC com repetição, backoff e verificações de saúde.
- **Painel TUI** — interface de terminal em tempo real, com várias telas e atalhos.
- **Configuração TOML** — perfis de moedas e opções centralizadas.
- **CLI** — comandos `mine`, `list-coins`, `benchmark` e `doctor`.

## Moedas compatíveis

| Moeda | Algoritmo | Método | Daemon |
|---|---|---|---|
| Monero (XMR) | RandomX | CPU (nativo) | `monerod` |
| Ethereum Classic (ETC) | Etchash | GPU (processo externo) | geth/open-etc |
| Ravencoin (RVN) | KawPow | GPU (processo externo) | `ravend` |
| Ergo (ERG) | Autolykos2 | GPU (processo externo) | ergo node |

## Instalação

### Pré-requisitos

- Rust 1.70 ou superior — [rustup.rs](https://rustup.rs/)
- Compilador C — MSVC Build Tools no Windows, GCC/Clang no Linux ou macOS
- CMake e Ninja — necessários para compilar a biblioteca C do RandomX

### Windows (MSYS2)

```powershell
winget install MSYS2.MSYS2
C:\msys64\usr\bin\bash.exe -lc "pacman -S --noconfirm --needed mingw-w64-x86_64-gcc mingw-w64-x86_64-cmake mingw-w64-x86_64-ninja"
$env:PATH = "C:\msys64\mingw64\bin;C:\msys64\usr\bin;$env:PATH"
```

### Linux

```bash
# Debian/Ubuntu
sudo apt install build-essential cmake ninja-build

# Arch
sudo pacman -S base-devel cmake ninja
```

### macOS

```bash
xcode-select --install
brew install cmake ninja
```

### Compilar o projeto

```bash
git clone https://github.com/Fernandorsoul/cryptominer.git
cd cryptominer

# Windows/MSYS2
export CMAKE_GENERATOR=Ninja

# Build de desenvolvimento
cargo build

# Build otimizado
cargo build --release

# Testes
cargo test
```

O binário de release fica em `target/release/cryptominer` (ou `cryptominer.exe` no Windows).

## Início rápido

### 1. Verifique o ambiente

```bash
cryptominer doctor
```

O comando verifica recursos do sistema, o arquivo de configuração e a conexão com o daemon.

### 2. Inicie um daemon Monero local

```bash
# Baixe em https://www.getmonero.org/downloads/
monerod --rpc-bind-ip 127.0.0.1 --rpc-bind-port 18081
```

### 3. Crie a configuração local

```bash
cp config.toml.example config.toml
# Edite config.toml e informe seu endereço de carteira.
```

O arquivo `config.toml` é local e não deve ser versionado se contiver dados sensíveis.

### 4. Execute

```bash
# CPU, com a carteira informada na linha de comando
cryptominer mine --wallet YOUR_WALLET_ADDRESS

# Limite de threads e intensidade
cryptominer mine --wallet YOUR_WALLET_ADDRESS --threads 6 --intensity 0.8

# Usa config.toml
cryptominer mine
```

### 5. Meça o desempenho

```bash
cryptominer benchmark --duration 30 --threads 6
```

## Referência da CLI

```text
cryptominer [OPTIONS] <COMMAND>

Commands:
  mine        Inicia a mineração
  list-coins  Lista moedas e algoritmos compatíveis
  benchmark   Executa benchmark de hashrate
  doctor      Verifica sistema e conexão com o daemon

Options:
  -c, --config <CONFIG>        Caminho da configuração [default: config.toml]
  -l, --log-level <LOG_LEVEL>  trace, debug, info, warn ou error
```

### Comando `mine`

```text
cryptominer mine [OPTIONS]

  -c, --coin <COIN>            Moeda; substitui a configuração
  -w, --wallet <WALLET>        Carteira que receberá as recompensas
      --daemon <DAEMON>        URL do daemon; substitui a configuração
  -t, --threads <THREADS>      Threads de CPU (0 = automático)
      --intensity <INTENSITY>  Intensidade entre 0.0 e 1.0
      --gpu                    Habilita mineração por GPU
```

## Configuração

Crie `config.toml` a partir de `config.toml.example`:

```toml
[mining]
coin = "monero"
wallet_address = "4AdUn..."
threads = 6                    # 0 = metade dos núcleos da CPU
intensity = 1.0                # 0.0 a 1.0; menor consome menos CPU

[daemon]
url = "http://127.0.0.1:18081"
user = ""
pass = ""

[gpu]
enabled = false
miner_path = "/usr/bin/lolminer"
devices = [0, 1]

[stratum]
enabled = false
bind = "0.0.0.0:3333"
```

Também é possível definir perfis para cada moeda:

```toml
[[profiles]]
name = "monero"
coin = "xmr"
algorithm = "randomx"
daemon_url = "http://127.0.0.1:18081"

[[profiles]]
name = "etc"
coin = "etc"
algorithm = "etchash"
daemon_url = "http://127.0.0.1:8551"
gpu_miner = "lolminer"
```

## Painel de terminal (TUI)

Durante a mineração, o projeto exibe métricas em tempo real: moeda, algoritmo, tempo de atividade, hashrate, shares aceitas/rejeitadas, estado do daemon e conexões Stratum.

| Tecla | Ação |
|---|---|
| `1` / `2` / `3` | Alterna entre Visão geral, Stratum e Ajuda |
| `Tab` / `→` | Próxima tela |
| `←` | Tela anterior |
| `q` / `Esc` | Sai do programa |
| `Ctrl+C` | Força a saída |

## Servidor Stratum

Para permitir conexões de mineradores externos, habilite o servidor integrado:

```toml
[stratum]
enabled = true
bind = "0.0.0.0:3333"
```

O endpoint é `stratum+tcp://YOUR_IP:3333`. O servidor busca templates de bloco, envia novos trabalhos aos clientes e processa submissões. Expor uma porta na rede demanda atenção às regras de firewall e à segurança da sua rede.

## Mineração em GPU

Instale um minerador externo compatível e informe seu caminho na configuração. Exemplos de backends suportados: lolMiner, TeamRedMiner, XMRig e comandos personalizados compatíveis com os argumentos esperados.

```bash
cryptominer mine --coin etc --gpu --daemon http://127.0.0.1:8551
```

O gerenciador detecta GPUs NVIDIA/AMD quando as ferramentas correspondentes estão disponíveis, reinicia processos em caso de falha e extrai métricas de saída. Monitore a temperatura e respeite os limites do seu hardware.

## Desenvolvimento

```bash
# Testes
cargo test

# Logs detalhados
RUST_LOG=debug cargo run -- mine --wallet test

# Formatação
cargo fmt

# Lint
cargo clippy
```

Principais crates: `clap`, `tokio`, `serde`, `reqwest`, `tracing`, `crossterm`, `rust-randomx`, `rayon` e `anyhow`.

## Licença

MIT

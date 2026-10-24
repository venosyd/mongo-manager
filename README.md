# Mongo Manager

Gerenciador de bancos MongoDB para desktop, escrito em Rust (egui + driver oficial do MongoDB).
É uma alternativa leve ao MongoDB Compass, com foco em **tabela de verdade**, **navegação rápida**
e **consultas com autocomplete**.

## Destaques

- **Tabela como visualização padrão**: colunas redimensionáveis (arraste a borda), larguras e colunas
  ocultas lembradas por coleção, cores por tipo BSON, tooltip com o valor completo e rolagem virtual.
- **Ordenação clicando no cabeçalho** (▲ → ▼ → sem ordem). Shift+clique adiciona campos à ordenação.
  A ordenação é feita no servidor, então vale para a coleção inteira e não só para a página.
- **Navegação**: primeira/anterior/próxima/**última** página, campo para ir direto a uma página,
  `Ctrl+End` leva ao último documento da coleção e `Ctrl+Home` ao primeiro. As setas atravessam
  páginas. Páginas da metade final são lidas com o sort invertido, então ir para o fim de uma
  coleção grande é tão rápido quanto abrir o começo.
- **Autocomplete contextual** (`Ctrl+Espaço` ou ao digitar): nomes de campos (inclusive aninhados,
  como `endereco.cidade`), operadores de consulta (`$gt`, `$in`…), de atualização (`$set`, `$inc`…),
  estágios de agregação (`$match`, `$group`…), expressões (`$sum`, `$dateToString`…) e referências
  `"$campo"`. Os campos vêm de uma amostra da coleção.
- **Sintaxe do mongosh** em todos os editores: chaves sem aspas, aspas simples, `ObjectId("…")`,
  `ISODate("…")`, `NumberLong(…)`, `NumberDecimal("…")`, `UUID("…")`, regex `/abc/i`, comentários
  e Extended JSON (`{"$oid": …}`). Erros indicam linha e coluna.
- **Edição**: duplo clique numa célula edita só aquele campo (`$set`/`$unset`, com opção "texto puro"
  para strings); `Enter` abre o documento inteiro; inserir (um, vários ou JSON Lines), clonar, excluir,
  `updateMany` e `deleteMany` com contagem prévia.
- **Agregação**: editor de pipeline com realce e autocomplete, paginação dos resultados, explain,
  pipelines salvos por coleção, inserção rápida de estágios.
- **Esquema**: presença e tipos de cada campo a partir de uma amostra.
- **Índices**: listar (tamanho, uso, propriedades), criar (único, sparse, TTL, parcial) e excluir.
- **Explain** com resumo: plano vencedor, índice usado, documentos/chaves examinados e alerta de COLLSCAN.
- **Importar/Exportar** JSON, JSON Lines e CSV (campos aninhados viram colunas `a.b`).
- Árvore de conexões → bancos → coleções com filtro, criar/renomear/excluir coleção, excluir banco,
  executar comandos (`db.runCommand`), histórico e favoritos de consultas, tema claro/escuro e zoom.

## Compilar e executar

Requer Rust (stable) e as bibliotecas gráficas usuais do Linux (Wayland ou X11).

```bash
cargo run --release
# ou instalar em ~/.cargo/bin:
cargo install --path .
mongo-manager
```

Compatível com MongoDB **4.0 ou superior** (testado com 4.0 e 7.0).

## Conexões

Use **➕ Nova conexão** e informe uma URI, por exemplo:

```
mongodb://usuario:senha@localhost:27017/?authSource=admin
mongodb+srv://usuario:senha@cluster0.exemplo.mongodb.net/
```

As conexões, o histórico, os pipelines salvos e as larguras de coluna ficam em
`~/.config/mongo-manager/config.json` (gravado com permissão `600`, pois a URI pode conter senha).

## Atalhos

| Atalho | Ação |
| --- | --- |
| `Enter` no filtro | Executar consulta (`Shift+Enter` quebra linha) |
| `Ctrl+Enter` | Executar pipeline / salvar no editor de documento |
| `Ctrl+Espaço` | Sugestões de autocomplete |
| `Ctrl+F` | Focar o filtro |
| `↑ ↓ PgUp PgDn` | Mover a seleção (atravessa páginas) |
| `Home` / `End` | Primeira / última linha da página |
| `Ctrl+Home` / `Ctrl+End` | Primeiro / último documento da coleção |
| `Alt+←` / `Alt+→` | Página anterior / próxima |
| `Enter` na tabela | Abrir o documento selecionado |
| `Delete` na tabela | Excluir o documento selecionado (`Enter` confirma) |
| `Ctrl+C` na tabela | Copiar o documento selecionado |
| `F5` | Recarregar a página |
| `Ctrl+W` / `Ctrl+Tab` | Fechar aba / próxima aba |
| `Ctrl +` / `Ctrl −` / `Ctrl 0` | Zoom |

## Estrutura do código

| Arquivo | Conteúdo |
| --- | --- |
| `src/shell.rs` | Parser e formatador da sintaxe do mongosh / Extended JSON |
| `src/mongo.rs` | Operações assíncronas no MongoDB (paginação, contagem, índices, import/export…) |
| `src/completion.rs` | Catálogo de operadores e lógica de sugestões por contexto |
| `src/editor.rs` | Editor com realce de sintaxe e popup de autocomplete |
| `src/grid.rs` | Tabela e lista JSON de documentos |
| `src/app.rs` | Estado da aplicação, árvore de conexões, abas |
| `src/views.rs` | Abas Documentos, Agregação, Esquema e Índices |
| `src/dialogs.rs` | Diálogos (conexão, edição, confirmações, índices, exportação…) |
| `src/script.rs` | Roteiro de testes de interface (desenvolvimento) |

## Desenvolvimento

```bash
cargo test                                            # testes unitários
MM_TEST_URI=mongodb://127.0.0.1:27018 cargo test      # + integração (cria e apaga o banco mm_itest)
```

Variáveis úteis para testar a interface: `MM_CONFIG` (outro arquivo de configuração), `MM_CONN` e
`MM_OPEN=banco.colecao` (conecta e abre a coleção ao iniciar), `MM_SHOT_DIR` (F12 salva screenshot)
e `MM_SCRIPT` (injeta cliques e teclas a partir de um roteiro; veja `src/script.rs`).

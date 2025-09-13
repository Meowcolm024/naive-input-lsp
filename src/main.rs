use dashmap::DashMap;
use std::collections::HashMap;
use std::str::Chars;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

#[derive(Debug, Clone)]
struct Keymap {
    here: Vec<String>,
    cont: HashMap<char, Keymap>,
}

impl Keymap {
    pub fn new(json: serde_json::Value) -> Self {
        Self::load(&json).unwrap_or(Keymap {
            here: vec![],
            cont: HashMap::new(),
        })
    }

    fn load(json: &serde_json::Value) -> Option<Self> {
        json.as_object().map(|obj| {
            let mut here = vec![];
            let mut cont = HashMap::new();
            if let Some(syms) = obj.get(">>").and_then(|a| a.as_array()) {
                syms.iter()
                    .for_each(|s| s.as_str().iter().for_each(|x| here.push(x.to_string())));
            }
            for (k, v) in obj {
                if k != ">>" {
                    if let Some(c) = k.chars().next() {
                        Self::load(v).into_iter().for_each(|z| {
                            cont.insert(c, z);
                        });
                    }
                }
            }
            Self { here, cont }
        })
    }

    pub fn lookup(&self, prefix: &str) -> Vec<String> {
        self.get(&mut prefix.chars())
    }

    fn get(&self, prefix: &mut Chars<'_>) -> Vec<String> {
        fn flatten(map: &HashMap<char, Keymap>) -> Vec<String> {
            let mut ret = vec![];
            for k in map.values() {
                ret.append(&mut k.here.clone());
                ret.append(&mut flatten(&k.cont));
            }
            ret
        }
        if let Some(c) = prefix.next() {
            self.cont.get(&c).map_or(vec![], |k| k.get(prefix))
        } else {
            let mut ret = self.here.clone();
            ret.append(&mut flatten(&self.cont));
            ret
        }
    }
}

#[derive(Debug)]
struct Backend {
    client: Client,
    keymap: Keymap,
    documents: DashMap<Url, String>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        self.client
            .log_message(MessageType::INFO, "aim server initialized!")
            .await;

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    // resolve_provider: Some(true),
                    trigger_characters: Some(('!'..='~').map(|s| s.to_string()).collect()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.documents
            .insert(params.text_document.uri, params.text_document.text);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        self.documents.insert(
            params.text_document.uri,
            params.content_changes[0].text.clone(),
        );
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let mut document = self.documents.get(&uri);

        let line = document
            .as_mut()
            .and_then(|d| d.lines().nth(position.line as usize))
            .map(|l| {
                l.chars()
                    .take(position.character as usize)
                    .collect::<String>()
            });

        let prefix = line.as_ref().and_then(|l| l.rsplit_once('\\'));

        if let Some((_, prefix)) = prefix {
            if prefix.len() == 0 {
                return Ok(None);
            }
            let completion_items: Vec<CompletionItem> = self
                .keymap
                .lookup(prefix)
                .into_iter()
                .map(|s| CompletionItem {
                    label: format!("{} {}", prefix, &s),
                    kind: Some(CompletionItemKind::TEXT),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                        range: Range {
                            start: Position {
                                line: position.line,
                                character: position.character - (prefix.len() as u32) - 1,
                            },
                            end: position,
                        },
                        new_text: s,
                    })),
                    ..Default::default()
                })
                .collect();

            self.client
                .log_message(MessageType::INFO, format!("Completion for {}", prefix))
                .await;

            Ok(Some(CompletionResponse::Array(completion_items)))
        } else {
            Ok(None)
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let raw = tokio::fs::read("keymap.json").await?;
    let keymap = Keymap::new(serde_json::from_str(
        std::str::from_utf8(&raw).unwrap_or(""),
    )?);

    let (service, socket) = LspService::new(|client| Backend {
        client,
        keymap,
        documents: DashMap::new(),
    });

    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;

    Ok(())
}

#[cfg(test)]
mod test {
    use crate::*;
    use tokio::io;

    #[test]
    fn test_lookup() -> io::Result<()> {
        let raw = std::fs::read("keymap.json")?;
        let json: serde_json::Value =
            serde_json::from_str(&std::string::String::from_utf8(raw).unwrap_or("".to_string()))?;
        let keymap = Keymap::new(json);
        assert_eq!(keymap.lookup("Gl-"), vec!["ƛ"]);
        Ok(())
    }
}

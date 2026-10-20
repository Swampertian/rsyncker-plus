use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::job::{Mode, Schedule};

const AUTOMATION_HELP: &str = "\
Uso por scripts e agentes:
  --json em qualquer comando gera saída em JSON no stdout; erros vão para o stderr.
  Código de saída: 0 = ok; 1 = erro ou confirmação pendente.
  Ações que podem apagar dados exigem --yes: remove, e add/edit em modo mirror.
  Veja o efeito antes de confirmar: preview <job>, ou add/edit com --dry-run.
  Host SSH novo: trust-host <job|usuário@host:/caminho> mostra a impressão digital;
  confirme com o dono do servidor e repita com --fingerprint <SHA256:...>.";

/// Gerenciador de jobs rsync agendados.
#[derive(Parser)]
#[command(version, after_help = AUTOMATION_HELP)]
pub struct Cli {
    /// Saída em JSON (para scripts e agentes).
    #[arg(long, global = true)]
    pub json: bool,
    /// Sem subcomando, abre a TUI.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Executa um job uma vez, mostrando os arquivos ao vivo.
    Run {
        /// Nome do job.
        job: String,
        /// Mostra progresso (%, velocidade, tempo restante) no stderr, se for terminal.
        #[arg(long)]
        progress: bool,
    },
    /// Mostra o que um job faria, sem copiar nem apagar nada (dry-run).
    Preview {
        /// Nome do job.
        job: String,
    },
    /// Observa a origem de um job e dispara runs quando algo muda (usado pelo systemd).
    Watch {
        /// Nome do job.
        job: String,
    },
    /// Cria um job a partir de um diretório de origem.
    Add {
        /// Diretório de origem (local, usuário@host:/caminho ou rsync://host/módulo).
        source: String,
        /// Diretório de destino (precisa existir; local ou remoto como a origem).
        #[arg(long)]
        to: String,
        /// copy: só adiciona/atualiza. mirror: apaga no destino o que sumiu da origem.
        #[arg(long, value_enum, default_value_t = Mode::Copy)]
        mode: Mode,
        /// Frequência do timer.
        #[arg(long, value_enum, default_value_t = Schedule::Daily)]
        schedule: Schedule,
        /// Nome do job (padrão: nome da pasta de origem).
        #[arg(long)]
        name: Option<String>,
        /// Padrão a ignorar (pode repetir).
        #[arg(long = "exclude")]
        excludes: Vec<String>,
        /// Limite de banda em KB/s.
        #[arg(long)]
        bwlimit: Option<u32>,
        /// Também roda quando algo muda na origem.
        #[arg(long)]
        on_change: bool,
        /// Chave SSH privada (ssh) ou arquivo de senha (rsync://) do lado remoto.
        #[arg(long)]
        key: Option<PathBuf>,
        #[command(flatten)]
        confirm: Confirm,
    },
    /// Altera um job existente; só muda o que for informado.
    Edit {
        /// Nome do job.
        job: String,
        #[command(flatten)]
        changes: Changes,
        #[command(flatten)]
        confirm: Confirm,
    },
    /// Mostra a configuração completa e o estado de um job.
    Show {
        /// Nome do job.
        job: String,
    },
    /// Lista os jobs com estado, último e próximo run.
    List,
    /// Remove um job e suas units (os arquivos no destino ficam).
    Remove {
        /// Nome do job.
        job: String,
        /// Confirma a remoção.
        #[arg(long)]
        yes: bool,
    },
    /// Ativa os gatilhos de um job.
    Enable {
        /// Nome do job.
        job: String,
    },
    /// Desativa os gatilhos de um job.
    Disable {
        /// Nome do job.
        job: String,
    },
    /// Mostra ou confirma a chave de um host SSH ainda desconhecido.
    TrustHost {
        /// Nome do job ou endereço remoto (usuário@host:/caminho, ssh://host:porta/caminho).
        target: String,
        /// Impressão digital conferida (SHA256:...); só grava a chave se for igual.
        #[arg(long)]
        fingerprint: Option<String>,
    },
    /// Busca dispositivos na rede (mDNS) e no ~/.ssh/config.
    Discover,
}

#[derive(Args, Debug, Default, Clone)]
pub struct Confirm {
    /// Mostra o que o job faria e não salva nada.
    #[arg(long)]
    pub dry_run: bool,
    /// Confirma ações que podem apagar arquivos no destino (modo mirror).
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args, Debug, Default, Clone)]
pub struct Changes {
    /// Nova origem.
    #[arg(long)]
    pub source: Option<String>,
    /// Novo destino.
    #[arg(long)]
    pub to: Option<String>,
    /// Novo modo.
    #[arg(long, value_enum)]
    pub mode: Option<Mode>,
    /// Nova frequência do timer.
    #[arg(long, value_enum)]
    pub schedule: Option<Schedule>,
    /// Substitui a lista de padrões ignorados (pode repetir).
    #[arg(long = "exclude")]
    pub excludes: Vec<String>,
    /// Remove todos os padrões ignorados.
    #[arg(long, conflicts_with = "excludes")]
    pub clear_excludes: bool,
    /// Limite de banda em KB/s (0 = sem limite).
    #[arg(long)]
    pub bwlimit: Option<u32>,
    /// Liga ou desliga o disparo quando algo muda na origem.
    #[arg(long, value_name = "true|false")]
    pub on_change: Option<bool>,
    /// Nova chave SSH ou arquivo de senha.
    #[arg(long)]
    pub key: Option<PathBuf>,
    /// Remove a chave/senha configurada.
    #[arg(long, conflicts_with = "key")]
    pub no_key: bool,
}

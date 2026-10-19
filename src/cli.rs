use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::job::{Mode, Schedule};

/// Gerenciador de jobs rsync agendados.
#[derive(Parser)]
#[command(version)]
pub struct Cli {
    /// Sem subcomando, abre a TUI.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Executa um job uma vez.
    Run {
        /// Nome do job.
        job: String,
    },
    /// Observa a origem de um job e dispara runs quando algo muda.
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
    },
    /// Lista os jobs com último e próximo run.
    List,
    /// Remove um job e suas units.
    Remove {
        /// Nome do job.
        job: String,
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
}

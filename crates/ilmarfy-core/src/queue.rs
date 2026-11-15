//! Fila de reprodução: ordem original, aleatório, repetir e "tocar a seguir".
//! Guarda só ids de faixa; o player de cada plataforma busca a faixa e toca.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Repeat {
    Off,
    All,
    One,
}

#[derive(Debug, Clone)]
pub struct PlayQueue {
    /// Faixas na ordem em que a fila foi montada
    items: Vec<String>,
    /// Ordem de reprodução (índices em `items`); difere de 0..n quando o aleatório está ligado
    order: Vec<usize>,
    /// Posição atual em `order`
    pos: usize,
    shuffle: bool,
    repeat: Repeat,
    rng: u64,
}

impl PlayQueue {
    /// Fila começando pela faixa `start` (índice em `ids`)
    pub fn new(ids: Vec<String>, start: usize) -> Self {
        let n = ids.len();
        Self {
            items: ids,
            order: (0..n).collect(),
            pos: start.min(n.saturating_sub(1)),
            shuffle: false,
            repeat: Repeat::Off,
            rng: seed_from_time(),
        }
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.rng = seed.max(1);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn current(&self) -> Option<&str> {
        self.order.get(self.pos).map(|&i| self.items[i].as_str())
    }

    /// Faixas na ordem em que vão tocar (a atual incluída)
    pub fn upcoming(&self) -> Vec<&str> {
        self.order[self.pos.min(self.order.len())..]
            .iter()
            .map(|&i| self.items[i].as_str())
            .collect()
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: Repeat) {
        self.repeat = repeat;
    }

    /// Avança ao fim da faixa. `user_skip` diferencia o botão "próxima" (que sai do "repetir uma")
    /// do fim natural da música.
    pub fn next(&mut self, user_skip: bool) -> Option<&str> {
        if self.items.is_empty() {
            return None;
        }
        if self.repeat == Repeat::One && !user_skip {
            return self.current();
        }
        if self.pos + 1 < self.order.len() {
            self.pos += 1;
        } else if self.repeat == Repeat::All || (self.repeat == Repeat::One && user_skip) {
            // Nova volta: no aleatório, embaralha de novo para não repetir a mesma sequência
            if self.shuffle {
                self.reshuffle_all();
            }
            self.pos = 0;
        } else {
            return None;
        }
        self.current()
    }

    pub fn previous(&mut self) -> Option<&str> {
        if self.pos > 0 {
            self.pos -= 1;
        } else if self.repeat == Repeat::All && !self.order.is_empty() {
            self.pos = self.order.len() - 1;
        }
        self.current()
    }

    /// Liga/desliga o aleatório mantendo a faixa atual tocando
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle || self.items.is_empty() {
            self.shuffle = on;
            return;
        }
        let current = self.order[self.pos];
        if on {
            let mut rest: Vec<usize> = (0..self.items.len()).filter(|&i| i != current).collect();
            self.shuffle_slice(&mut rest);
            self.order = std::iter::once(current).chain(rest).collect();
            self.pos = 0;
        } else {
            self.order = (0..self.items.len()).collect();
            self.pos = current;
        }
        self.shuffle = on;
    }

    /// Pula direto para a faixa (primeira ocorrência na ordem de reprodução)
    pub fn jump_to(&mut self, track_id: &str) -> bool {
        match self.order.iter().position(|&i| self.items[i] == track_id) {
            Some(p) => {
                self.pos = p;
                true
            }
            None => false,
        }
    }

    /// "Tocar a seguir": entra logo depois da atual
    pub fn play_next(&mut self, track_id: String) {
        self.items.push(track_id);
        let idx = self.items.len() - 1;
        let at = if self.order.is_empty() { 0 } else { self.pos + 1 };
        self.order.insert(at, idx);
    }

    /// "Adicionar à fila": entra no fim
    pub fn append(&mut self, track_id: String) {
        self.items.push(track_id);
        self.order.push(self.items.len() - 1);
    }

    /// Remove uma faixa da fila (todas as ocorrências); a atual não pode ser removida
    pub fn remove(&mut self, track_id: &str) {
        let current = self.order.get(self.pos).copied();
        if current.is_some_and(|c| self.items[c] == track_id) {
            return;
        }
        let keep: Vec<bool> = self.items.iter().map(|id| id != track_id).collect();
        let mut remap = vec![usize::MAX; self.items.len()];
        let mut new_items = Vec::new();
        for (i, id) in self.items.iter().enumerate() {
            if keep[i] {
                remap[i] = new_items.len();
                new_items.push(id.clone());
            }
        }
        let cur_new = current.map(|c| remap[c]);
        self.order = self.order.iter().filter(|&&i| keep[i]).map(|&i| remap[i]).collect();
        self.items = new_items;
        self.pos = cur_new.and_then(|c| self.order.iter().position(|&i| i == c)).unwrap_or(0);
    }

    fn reshuffle_all(&mut self) {
        let mut all: Vec<usize> = (0..self.items.len()).collect();
        self.shuffle_slice(&mut all);
        // Evita repetir a última faixa logo no começo da nova volta
        if all.len() > 1 && Some(&all[0]) == self.order.last() {
            all.swap(0, 1);
        }
        self.order = all;
    }

    /// Fisher-Yates com xorshift (sem dependências; determinístico com `with_seed`)
    fn shuffle_slice(&mut self, v: &mut [usize]) {
        for i in (1..v.len()).rev() {
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            let j = (self.rng % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
    }
}

fn seed_from_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(n: usize, start: usize) -> PlayQueue {
        PlayQueue::new((0..n).map(|i| format!("t{i}")).collect(), start).with_seed(42)
    }

    #[test]
    fn ordem_normal_e_fim_da_fila() {
        let mut fila = q(3, 1);
        assert_eq!(fila.current(), Some("t1"));
        assert_eq!(fila.next(false), Some("t2"));
        assert_eq!(fila.next(false), None, "sem repetir, acaba no fim");
        assert_eq!(fila.previous(), Some("t1"));
    }

    #[test]
    fn repetir_tudo_e_uma() {
        let mut fila = q(2, 1);
        fila.set_repeat(Repeat::All);
        assert_eq!(fila.next(false), Some("t0"));
        fila.set_repeat(Repeat::One);
        assert_eq!(fila.next(false), Some("t0"), "repetir uma fica na mesma no fim natural");
        assert_eq!(fila.next(true), Some("t1"), "mas o botão próxima avança");
    }

    #[test]
    fn aleatorio_mantem_a_atual_e_toca_todas() {
        let mut fila = q(10, 4);
        fila.set_shuffle(true);
        assert_eq!(fila.current(), Some("t4"));
        let mut tocadas = vec![fila.current().unwrap().to_string()];
        while let Some(t) = fila.next(false) {
            tocadas.push(t.to_string());
        }
        tocadas.sort();
        let mut todas: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
        todas.sort();
        assert_eq!(tocadas, todas, "cada faixa toca exatamente uma vez");

        fila.set_shuffle(false);
        assert!(fila.current().is_some());
    }

    #[test]
    fn desligar_aleatorio_volta_para_a_ordem_original() {
        let mut fila = q(5, 0);
        fila.set_shuffle(true);
        fila.next(false);
        let atual = fila.current().unwrap().to_string();
        fila.set_shuffle(false);
        assert_eq!(fila.current(), Some(atual.as_str()));
        let n: usize = atual[1..].parse().unwrap();
        assert_eq!(fila.upcoming().len(), 5 - n);
    }

    #[test]
    fn tocar_a_seguir_adicionar_remover_e_pular() {
        let mut fila = q(3, 0);
        fila.play_next("x".into());
        fila.append("y".into());
        assert_eq!(fila.upcoming(), vec!["t0", "x", "t1", "t2", "y"]);
        fila.remove("t1");
        fila.remove("t0"); // a atual não sai
        assert_eq!(fila.upcoming(), vec!["t0", "x", "t2", "y"]);
        assert!(fila.jump_to("t2"));
        assert_eq!(fila.current(), Some("t2"));
        assert!(!fila.jump_to("nao-existe"));
    }

    #[test]
    fn fila_vazia() {
        let mut fila = PlayQueue::new(vec![], 0);
        assert!(fila.current().is_none() && fila.next(true).is_none() && fila.previous().is_none());
        fila.set_shuffle(true);
        fila.append("a".into());
        assert_eq!(fila.current(), Some("a"));
    }
}

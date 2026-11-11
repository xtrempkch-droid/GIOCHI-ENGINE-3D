//! *Interest Management* — o host envia cada inimigo só a quem pode vê-lo.
//!
//! ## Por que isso é obrigatório, em número
//!
//! O relatório traz a conta: mapa de 4×4 telas, ~150 inimigos, 4 clientes.
//!
//! ```text
//! sem AOI:  150 inimigos × 5 bytes × 15 Hz × 4 clientes = 45 KB/s de upload
//! com AOI:  ~13 inimigos × 5 bytes × 15 Hz × 4 clientes =  4 KB/s
//! ```
//!
//! Num host ARM64 portátil alimentado por bateria e com rádio SDIO, 45 KB/s de
//! upload contínuo aparece como engasgo de framerate — e o culpado parece a
//! GPU. É a diferença entre um jogo que roda e um que "trava sem motivo".
//!
//! ## O raio é parâmetro, não constante da engine
//!
//! O relatório diz "1,5 × meia-diagonal da tela do cliente". Essa fórmula
//! depende de **quanto do mundo o jogador vê**, que é decisão do jogo (zoom de
//! câmera, FOV, se o mapa tem névoa). A engine não pode saber isso.
//!
//! Então [`FiltroAoi`] recebe o raio pronto e só aplica. Quem calcula é o jogo,
//! que é dono da câmera.

use crate::wire::SnapshotInimigo;

/// Área de interesse de **um** cliente.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AreaDeInteresse {
    /// Centro, em coordenadas de mundo.
    pub x: f32,
    /// Centro, em coordenadas de mundo.
    pub z: f32,
    /// Raio em unidades de mundo. Zero desliga o filtro (envia tudo).
    pub raio: f32,
}

impl AreaDeInteresse {
    /// Cria a área. Raio negativo vira zero, e `NaN` desliga o filtro.
    pub fn nova(x: f32, z: f32, raio: f32) -> Self {
        Self {
            x,
            z,
            raio: if raio.is_nan() { 0.0 } else { raio.max(0.0) },
        }
    }

    /// Atalho para o relatório: `1,5 ×` meia-diagonal da tela em unidades de
    /// mundo.
    ///
    /// O fator 1,5 é folga para o inimigo entrar na tela antes de ser
    /// desenhado — sem ela, um inimigo aparece na borda com um tick de atraso e
    /// o jogador vê um "salto".
    pub fn da_tela(centro_x: f32, centro_z: f32, meia_diagonal: f32) -> Self {
        Self::nova(centro_x, centro_z, meia_diagonal * 1.5)
    }

    /// Distância ao quadrado. Comparar quadrado evita `sqrt` por inimigo por
    /// cliente por tick — que a 150 × 4 × 15 = 9000 vezes por segundo seria
    /// desperdício mensurável num Cortex-A35.
    fn distancia2(&self, x: f32, z: f32) -> f32 {
        let dx = x - self.x;
        let dz = z - self.z;
        dx * dx + dz * dz
    }

    /// Um inimigo está dentro?
    pub fn contem(&self, inimigo: &SnapshotInimigo) -> bool {
        if self.raio <= 0.0 {
            return true; // raio zero = sem filtro, não "nada visível"
        }
        self.distancia2(inimigo.x, inimigo.z) <= self.raio * self.raio
    }
}

/// Filtra o snapshot de inimigos para um cliente.
///
/// Devolve quantos foram mantidos e quantos foram cortados, para o host poder
/// registrar a taxa sem recalcular nada.
pub fn filtrar(
    inimigos: &[SnapshotInimigo],
    area: AreaDeInteresse,
    destino: &mut Vec<SnapshotInimigo>,
) -> (usize, usize) {
    destino.clear();
    destino.extend(inimigos.iter().copied().filter(|i| area.contem(i)));
    (destino.len(), inimigos.len() - destino.len())
}

/// Filtro com buffer reaproveitado, para o host não alocar a cada snapshot.
#[derive(Debug, Default)]
pub struct FiltroAoi {
    buffer: Vec<SnapshotInimigo>,
}

impl FiltroAoi {
    /// Novo filtro, sem alocação até o primeiro uso.
    pub fn novo() -> Self {
        Self::default()
    }

    /// Filtra e devolve a lista já no buffer interno do filtro.
    pub fn filtrar(
        &mut self,
        inimigos: &[SnapshotInimigo],
        area: AreaDeInteresse,
    ) -> &[SnapshotInimigo] {
        filtrar(inimigos, area, &mut self.buffer);
        &self.buffer
    }

    /// Quantos inimigos saíram no último [`Self::filtrar`].
    pub fn ultimo_tamanho(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn inimigo(id: u16, x: f32, z: f32) -> SnapshotInimigo {
        SnapshotInimigo {
            id,
            x,
            z,
            hp: 10,
            tipo: 0,
            fase_chefe: 0,
        }
    }

    #[test]
    fn cliente_no_centro_ve_a_regiao_dele_e_nao_o_mundo() {
        let mundo: Vec<_> = (0..150)
            .map(|i| inimigo(i, (i % 15) as f32 * 10.0, (i / 15) as f32 * 10.0))
            .collect();

        let area = AreaDeInteresse::nova(30.0, 30.0, 25.0);
        let mut saida = Vec::new();
        let (mantidos, cortados) = filtrar(&mundo, area, &mut saida);

        assert!(mantidos > 0, "quem está no centro tem de ver algo");
        assert!(cortados > 0, "o mundo é maior que a área");
        assert!(mantidos < mundo.len());
        assert_eq!(mantidos + cortados, mundo.len());
    }

    #[test]
    fn cliente_fora_recebe_zero_quando_nada_esta_na_area() {
        let mundo = vec![inimigo(1, 0.0, 0.0), inimigo(2, 1.0, 1.0)];
        let area = AreaDeInteresse::nova(1000.0, 1000.0, 25.0);
        let mut saida = Vec::new();
        let (mantidos, cortados) = filtrar(&mundo, area, &mut saida);
        assert_eq!(mantidos, 0);
        assert_eq!(cortados, 2);
    }

    #[test]
    fn borda_e_inclusiva() {
        let dentro = inimigo(1, 10.0, 0.0);
        let area = AreaDeInteresse::nova(0.0, 0.0, 10.0);
        assert!(area.contem(&dentro), "raio exato pertence à área");

        let fora = inimigo(2, 10.01, 0.0);
        assert!(!area.contem(&fora));
    }

    #[test]
    fn raio_zero_desliga_o_filtro_em_vez_de_esconder_tudo() {
        // "Sem filtro" e "nada visível" precisam ser distintos: raio 0 é o
        // primeiro. Se fosse o segundo, um cliente mal configurado veria um
        // jogo vazio e ninguém saberia por quê.
        let mundo = vec![inimigo(1, 999.0, 999.0)];
        let area = AreaDeInteresse::nova(0.0, 0.0, 0.0);
        let mut saida = Vec::new();
        let (mantidos, _) = filtrar(&mundo, area, &mut saida);
        assert_eq!(mantidos, 1);
    }

    #[test]
    fn raio_negativo_ou_nan_nao_produzem_area_invalida() {
        assert_eq!(AreaDeInteresse::nova(0.0, 0.0, -5.0).raio, 0.0);
        assert_eq!(AreaDeInteresse::nova(0.0, 0.0, f32::NAN).raio, 0.0);
    }

    #[test]
    fn formula_da_tela_aplica_folga_de_um_e_meio() {
        let area = AreaDeInteresse::da_tela(0.0, 0.0, 10.0);
        assert!((area.raio - 15.0).abs() < 1e-6);
    }

    #[test]
    fn buffer_e_reaproveitado_entre_chamadas() {
        let mut filtro = FiltroAoi::novo();
        let mundo = vec![inimigo(1, 0.0, 0.0), inimigo(2, 50.0, 50.0)];

        let area_perto = AreaDeInteresse::nova(0.0, 0.0, 5.0);
        assert_eq!(filtro.filtrar(&mundo, area_perto).len(), 1);

        // Segunda chamada com resultado maior: o buffer cresce sem duplicar o
        // conteúdo anterior.
        let area_longe = AreaDeInteresse::nova(1000.0, 1000.0, 5.0);
        assert_eq!(filtro.filtrar(&mundo, area_longe).len(), 0);

        let sem_filtro = AreaDeInteresse::nova(0.0, 0.0, 0.0);
        assert_eq!(filtro.filtrar(&mundo, sem_filtro).len(), 2);
        assert_eq!(filtro.ultimo_tamanho(), 2);
    }

    #[test]
    fn a_contagem_do_relatorio_se_sustenta() {
        // A conta que justifica o AOI: 150 inimigos em 16 telas, área de 1 tela.
        // Esperado ~1/16 do mundo por cliente.
        let mundo: Vec<_> = (0..150)
            .map(|i| {
                let col = (i % 16) as f32;
                let lin = (i / 16) as f32;
                inimigo(i, col * 100.0, lin * 100.0)
            })
            .collect();

        let area = AreaDeInteresse::nova(0.0, 0.0, 70.0);
        let mut saida = Vec::new();
        let (mantidos, _) = filtrar(&mundo, area, &mut saida);

        // Aproximadamente 1/16 de 150 ≈ 9. Aceita uma faixa larga: o importante
        // é que seja ordem de grandeza menor, não a fração exata.
        assert!(mantidos <= 20, "AOI devia reduzir muito, veio {mantidos}");
        assert!(mantidos >= 3, "e não zerar, veio {mantidos}");
    }
}

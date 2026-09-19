//! French civil saint of the day (calendrier des fêtes).
//!
//! One principal name per date, in the kitchen-calendar form
//! `Sainte Nadège` / `Saint Nicolas`. Fixed feasts (Noël, Toussaint)
//! keep their usual names. Leap day is Saint Auguste.

use chrono::{Datelike, NaiveDate};

use super::contribute::{Contribution, SourceOutcome};
use super::context::SourceContext;
use super::DataSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaintDay {
    pub title: &'static str,
    pub name: &'static str,
}

const fn st(name: &'static str) -> SaintDay {
    SaintDay {
        title: "Saint",
        name,
    }
}

const fn ste(name: &'static str) -> SaintDay {
    SaintDay {
        title: "Sainte",
        name,
    }
}

const fn sts(name: &'static str) -> SaintDay {
    SaintDay {
        title: "Saints",
        name,
    }
}

const fn feast(name: &'static str) -> SaintDay {
    SaintDay { title: "", name }
}

const JAN: &[SaintDay] = &[
    ste("Marie"),
    st("Basile"),
    ste("Geneviève"),
    st("Odilon"),
    st("Édouard"),
    st("Mélaine"),
    st("Raymond"),
    st("Lucien"),
    ste("Alix"),
    st("Guillaume"),
    ste("Pauline"),
    ste("Tatiana"),
    ste("Yvette"),
    ste("Nina"),
    st("Rémi"),
    st("Marcel"),
    ste("Roseline"),
    ste("Prisca"),
    st("Marius"),
    st("Sébastien"),
    ste("Agnès"),
    st("Vincent"),
    st("Barnard"),
    st("François de Sales"),
    st("Paul"),
    ste("Paule"),
    ste("Angèle"),
    st("Thomas d'Aquin"),
    st("Gildas"),
    ste("Martine"),
    ste("Marcelle"),
];

const FEB: &[SaintDay] = &[
    ste("Ella"),
    feast("Chandeleur"),
    st("Blaise"),
    ste("Véronique"),
    ste("Agathe"),
    st("Gaston"),
    ste("Eugénie"),
    ste("Jacqueline"),
    ste("Apolline"),
    st("Arnaud"),
    feast("Notre-Dame de Lourdes"),
    st("Félix"),
    ste("Béatrice"),
    st("Valentin"),
    st("Claude"),
    ste("Julienne"),
    st("Alexis"),
    ste("Bernadette"),
    st("Gabin"),
    ste("Aimée"),
    st("Pierre-Damien"),
    ste("Isabelle"),
    st("Lazare"),
    st("Modeste"),
    st("Roméo"),
    st("Nestor"),
    ste("Honorine"),
    st("Romain"),
    st("Auguste"),
];

const MAR: &[SaintDay] = &[
    st("Aubin"),
    st("Charles"),
    st("Guénolé"),
    st("Casimir"),
    ste("Olive"),
    ste("Colette"),
    ste("Félicité"),
    st("Jean de Dieu"),
    ste("Françoise"),
    st("Vivien"),
    ste("Rosine"),
    ste("Justine"),
    st("Rodrigue"),
    ste("Mathilde"),
    ste("Louise"),
    ste("Bénédicte"),
    st("Patrice"),
    st("Cyrille"),
    st("Joseph"),
    st("Herbert"),
    ste("Clémence"),
    ste("Léa"),
    st("Victorien"),
    ste("Catherine"),
    feast("Annonciation"),
    ste("Larissa"),
    st("Habib"),
    st("Gontran"),
    ste("Gwladys"),
    st("Amédée"),
    st("Benjamin"),
];

const APR: &[SaintDay] = &[
    st("Hugues"),
    ste("Sandrine"),
    st("Richard"),
    st("Isidore"),
    ste("Irène"),
    st("Marcellin"),
    st("Jean-Baptiste de La Salle"),
    ste("Julie"),
    st("Gautier"),
    st("Fulbert"),
    st("Stanislas"),
    st("Jules"),
    ste("Ida"),
    st("Maxime"),
    st("Paterne"),
    st("Benoît-Joseph"),
    st("Anicet"),
    st("Parfait"),
    ste("Emma"),
    ste("Odette"),
    st("Anselme"),
    st("Alexandre"),
    st("Georges"),
    st("Fidèle"),
    st("Marc"),
    ste("Alida"),
    ste("Zita"),
    ste("Valérie"),
    ste("Catherine de Sienne"),
    st("Robert"),
];

const MAY: &[SaintDay] = &[
    st("Joseph"),
    st("Boris"),
    sts("Philippe et Jacques"),
    st("Sylvain"),
    ste("Judith"),
    ste("Prudence"),
    ste("Gisèle"),
    st("Désiré"),
    st("Pacôme"),
    ste("Solange"),
    ste("Estelle"),
    st("Achille"),
    ste("Rolande"),
    st("Matthias"),
    ste("Denise"),
    st("Honoré"),
    st("Pascal"),
    st("Éric"),
    st("Yves"),
    st("Bernardin"),
    st("Constantin"),
    st("Émile"),
    st("Didier"),
    st("Donatien"),
    ste("Sophie"),
    st("Bérenger"),
    st("Augustin"),
    st("Germain"),
    st("Aymard"),
    st("Ferdinand"),
    ste("Perrine"),
];

const JUN: &[SaintDay] = &[
    st("Justin"),
    ste("Blandine"),
    st("Kévin"),
    ste("Clotilde"),
    st("Igor"),
    st("Norbert"),
    st("Gilbert"),
    st("Médard"),
    ste("Diane"),
    st("Landry"),
    st("Barnabé"),
    st("Guy"),
    st("Antoine de Padoue"),
    st("Élisée"),
    ste("Germaine"),
    st("Jean-François Régis"),
    st("Hervé"),
    st("Léonce"),
    st("Romuald"),
    st("Silvère"),
    st("Rodolphe"),
    st("Alban"),
    ste("Audrey"),
    st("Jean-Baptiste"),
    st("Prosper"),
    st("Anthelme"),
    st("Fernand"),
    st("Irénée"),
    sts("Pierre et Paul"),
    st("Martial"),
];

const JUL: &[SaintDay] = &[
    st("Thierry"),
    st("Martinien"),
    st("Thomas"),
    st("Florent"),
    st("Antoine"),
    ste("Mariette"),
    st("Raoul"),
    st("Thibault"),
    ste("Amandine"),
    st("Ulrich"),
    st("Benoît"),
    st("Olivier"),
    st("Henri"),
    st("Camille"),
    st("Donald"),
    ste("Elvire"),
    ste("Charlotte"),
    st("Frédéric"),
    st("Arsène"),
    ste("Marina"),
    st("Victor"),
    ste("Marie-Madeleine"),
    ste("Brigitte"),
    ste("Christine"),
    st("Jacques"),
    ste("Anne"),
    ste("Nathalie"),
    st("Samson"),
    ste("Marthe"),
    ste("Juliette"),
    st("Ignace"),
];

const AUG: &[SaintDay] = &[
    st("Alphonse"),
    st("Julien"),
    ste("Lydie"),
    st("Jean-Marie Vianney"),
    st("Abel"),
    st("Octavien"),
    st("Gaétan"),
    st("Dominique"),
    st("Amour"),
    st("Laurent"),
    ste("Claire"),
    ste("Clarisse"),
    st("Hippolyte"),
    st("Evrard"),
    feast("Assomption"),
    st("Armel"),
    st("Hyacinthe"),
    ste("Hélène"),
    st("Jean Eudes"),
    st("Bernard"),
    st("Christophe"),
    st("Fabrice"),
    ste("Rose"),
    st("Barthélemy"),
    st("Louis"),
    ste("Natacha"),
    ste("Monique"),
    st("Augustin"),
    ste("Sabine"),
    st("Fiacre"),
    st("Aristide"),
];

const SEP: &[SaintDay] = &[
    st("Gilles"),
    ste("Ingrid"),
    st("Grégoire"),
    ste("Rosalie"),
    ste("Raïssa"),
    st("Bertrand"),
    ste("Reine"),
    st("Adrien"),
    st("Alain"),
    ste("Inès"),
    st("Adelphe"),
    st("Apollinaire"),
    st("Aimé"),
    st("Lubin"),
    st("Roland"),
    ste("Édith"),
    st("Renaud"),
    ste("Nadège"),
    ste("Émilie"),
    st("Davy"),
    st("Matthieu"),
    st("Maurice"),
    st("Constant"),
    ste("Thècle"),
    st("Hermann"),
    sts("Côme et Damien"),
    st("Vincent de Paul"),
    st("Venceslas"),
    st("Michel"),
    st("Jérôme"),
];

const OCT: &[SaintDay] = &[
    ste("Thérèse"),
    st("Léger"),
    st("Gérard"),
    st("François d'Assise"),
    ste("Fleur"),
    st("Bruno"),
    st("Serge"),
    ste("Pélagie"),
    st("Denis"),
    st("Ghislain"),
    st("Firmin"),
    st("Wilfried"),
    st("Géraud"),
    st("Juste"),
    ste("Thérèse d'Avila"),
    ste("Edwige"),
    st("Baudouin"),
    st("Luc"),
    st("René"),
    ste("Adeline"),
    ste("Céline"),
    ste("Élodie"),
    st("Jean de Capistran"),
    st("Florentin"),
    st("Crépin"),
    st("Dimitri"),
    ste("Émeline"),
    st("Simon"),
    st("Narcisse"),
    st("Bienvenu"),
    st("Quentin"),
];

const NOV: &[SaintDay] = &[
    feast("Toussaint"),
    feast("Jour des défunts"),
    st("Hubert"),
    st("Charles"),
    ste("Sylvie"),
    ste("Bertille"),
    ste("Carine"),
    st("Geoffroy"),
    st("Théodore"),
    st("Léon"),
    st("Martin"),
    st("Christian"),
    st("Brice"),
    st("Sidoine"),
    st("Albert"),
    ste("Marguerite"),
    ste("Élisabeth"),
    ste("Aude"),
    st("Tanguy"),
    st("Edmond"),
    feast("Présentation de Marie"),
    ste("Cécile"),
    st("Clément"),
    ste("Flora"),
    ste("Catherine"),
    ste("Delphine"),
    st("Séverin"),
    st("Jacques"),
    st("Saturnin"),
    st("André"),
];

const DEC: &[SaintDay] = &[
    ste("Florence"),
    ste("Viviane"),
    st("François-Xavier"),
    ste("Barbara"),
    st("Gérald"),
    st("Nicolas"),
    st("Ambroise"),
    feast("Immaculée Conception"),
    st("Pierre Fourier"),
    st("Romaric"),
    st("Daniel"),
    ste("Jeanne de Chantal"),
    ste("Lucie"),
    ste("Odile"),
    ste("Ninon"),
    ste("Alice"),
    st("Gaël"),
    st("Gatien"),
    st("Urbain"),
    st("Théophile"),
    st("Pierre"),
    ste("Françoise-Xavière"),
    st("Armand"),
    ste("Adèle"),
    feast("Noël"),
    st("Étienne"),
    st("Jean"),
    sts("Innocents"),
    st("David"),
    st("Roger"),
    st("Sylvestre"),
];

const SAINTS: [&[SaintDay]; 12] = [JAN, FEB, MAR, APR, MAY, JUN, JUL, AUG, SEP, OCT, NOV, DEC];

pub struct SaintsSource;

#[async_trait::async_trait]
impl DataSource for SaintsSource {
    fn id(&self) -> &'static str {
        "saints"
    }

    fn enabled(&self, _cfg: &crate::config::Config) -> bool {
        true
    }

    async fn load(&self, ctx: &SourceContext<'_>) -> anyhow::Result<SourceOutcome> {
        let saint = of_date(ctx.today);
        Ok(SourceOutcome::live(
            String::new(),
            Contribution::Mast {
                saint_title: saint.title.to_string(),
                saint_name: saint.name.to_string(),
            },
        ))
    }
}

pub fn of_date(date: NaiveDate) -> SaintDay {
    let month = date.month() as usize;
    let day = date.day() as usize;
    SAINTS
        .get(month.saturating_sub(1))
        .and_then(|days| days.get(day.saturating_sub(1)))
        .copied()
        .unwrap_or(feast(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_covers_every_civil_day() {
        assert_eq!(JAN.len(), 31);
        assert_eq!(FEB.len(), 29);
        assert_eq!(MAR.len(), 31);
        assert_eq!(APR.len(), 30);
        assert_eq!(MAY.len(), 31);
        assert_eq!(JUN.len(), 30);
        assert_eq!(JUL.len(), 31);
        assert_eq!(AUG.len(), 31);
        assert_eq!(SEP.len(), 30);
        assert_eq!(OCT.len(), 31);
        assert_eq!(NOV.len(), 30);
        assert_eq!(DEC.len(), 31);
        for month in SAINTS {
            for day in month {
                assert!(!day.name.is_empty());
            }
        }
    }

    #[test]
    fn eighteenth_of_september_is_sainte_nadege() {
        let saint = of_date(NaiveDate::from_ymd_opt(2026, 9, 18).unwrap());
        assert_eq!(saint, ste("Nadège"));
    }

    #[test]
    fn leap_day_is_saint_auguste() {
        let saint = of_date(NaiveDate::from_ymd_opt(2024, 2, 29).unwrap());
        assert_eq!(saint, st("Auguste"));
    }

    #[test]
    fn well_known_feasts() {
        assert_eq!(
            of_date(NaiveDate::from_ymd_opt(2026, 12, 6).unwrap()),
            st("Nicolas")
        );
        assert_eq!(
            of_date(NaiveDate::from_ymd_opt(2026, 12, 25).unwrap()),
            feast("Noël")
        );
        assert_eq!(
            of_date(NaiveDate::from_ymd_opt(2026, 11, 1).unwrap()),
            feast("Toussaint")
        );
        assert_eq!(
            of_date(NaiveDate::from_ymd_opt(2026, 1, 3).unwrap()),
            ste("Geneviève")
        );
    }
}

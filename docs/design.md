# Dossier de design — CleanSlate

Dernière mise à jour : 29 oct. 2026

## 1. Maquettes

Les huit planches sont dans [`docs/maquettes/`](maquettes/index.html) : ouvrir `index.html` dans un navigateur, puis cliquer d'un écran à l'autre. Ces fichiers HTML sont la référence ; le brief de la section 7 permet de régénérer les planches dans un outil de design comme Canva.

| Planche | Fichier | Écran | Exigences couvertes |
| --- | --- | --- | --- |
| Accueil | `01-accueil.html` | Tableau de bord, bandeau « Mode simulation », dernières sessions | EF-20, ENF-04 |
| Applications | `02-applications.html` | Liste, recherche, tri, filtre système, barre d'action « Désinstaller complètement » | EF-01 à EF-03, EF-12 |
| Restes orphelins | `03-restes-orphelins.html` | Groupes par application déduite, cases à cocher, badges de confiance, raisons, barre de sélection collante | EF-05 à EF-11 |
| Assistant de désinstallation | `04-assistant.html` | 4 étapes, résultat du désinstalleur, restes de l'application | EF-12 à EF-15 |
| Confirmation | `05-confirmation.html` | Dialogue récapitulatif avant quarantaine | EF-16, ENF-02 |
| Historique | `06-historique.html` | Sessions, restauration partielle ou totale, date de purge | EF-18, EF-19 |
| Réglages | `07-reglages.html` | Simulation, durée de quarantaine, exclusions, thème, journaux | EF-19, EF-20, ENF-08 |
| Charte graphique | `08-charte.html` | Couleurs, typographie, badges, boutons | — |

## 2. Principes de design

L'interface doit rassurer avant tout : l'utilisateur supprime des choses qu'il ne connaît pas toujours.

1. **Expliquer chaque proposition.** Chaque élément affiche son chemin complet et la raison pour laquelle il est proposé (« Aucune application de cet éditeur installée »).
2. **Le rouge signale l'irréversible, et seulement lui.** La couleur danger est réservée aux boutons qui déplacent ou suppriment ; le reste utilise l'accent sarcelle.
3. **Prudence par défaut.** Seule la confiance « Élevée » est précochée ; le mode simulation est actif au premier lancement et rappelé par un bandeau.
4. **Toujours un retour possible.** Le dialogue de confirmation et l'Historique rappellent la quarantaine de 30 jours et la restauration.
5. **Natif Windows 11.** Volet de navigation, matériau Mica, icônes Segoe Fluent, contrôles WinUI standards : pas de composants maison si WinUI en fournit un.

## 3. Jetons de design

Ces valeurs sont des ressources XAML (`ThemeDictionaries` Light / Dark dans `app/CleanSlate.App/App.xaml`, en place depuis J1) ; aucune couleur n'est écrite en dur dans une vue. Chaque badge de confiance utilise deux ressources, `…ForegroundBrush` (texte) et `…BackgroundBrush` (fond), par exemple `ConfidenceHighForegroundBrush`. Les valeurs sombres n'ont pas encore de maquette et sont à valider à l'écran ; les ressources `HighContrast` arrivent en J6.

| Jeton | Clair | Sombre | Usage |
| --- | --- | --- | --- |
| `AccentBrush` | #0F766E | #4FB8AD | Boutons principaux, sélection, liens |
| `AccentHoverBrush` | #115E59 | #6CCBC1 | Survol |
| `AccentSubtleBrush` | #E3F2F1 | #1E3A38 | Bandeaux d'info, ligne sélectionnée |
| `OnAccentBrush` | #FFFFFF | #0B2B28 | Texte sur accent |
| `WindowBackgroundBrush` | #F3F3F3 | #202020 | Fond fenêtre (Mica) |
| `SurfaceBrush` | #FFFFFF | #2B2B2B | Cartes, listes |
| `BorderBrush` | #E5E5E5 | #3A3A3A | Bordures de cartes |
| `ControlBorderBrush` | #D1D1D1 | #4A4A4A | Champs, boutons secondaires |
| `TextPrimaryBrush` | #1A1A1A | #FFFFFF | Texte |
| `TextSecondaryBrush` | #5C5C5C | #C5C5C5 | Légendes, métadonnées |
| `DangerBrush` | #C42B1C | #E0473A | Boutons de suppression uniquement |
| `ConfidenceHigh…Brush` (texte / fond) | #0E700E / #DFF6DD | #6CCB5F / #393D1B | Badge « Élevée » |
| `ConfidenceMedium…Brush` (texte / fond) | #8A5100 / #FFF4CE | #FCE100 / #433519 | Badge « Moyenne » |
| `ConfidenceLow…Brush` (texte / fond) | #5C5C5C / #EDEDED | #C5C5C5 / #3A3A3A | Badge « Faible » |

**Typographie** : Segoe UI Variable via les styles WinUI (`TitleTextBlockStyle` 28/600, `SubtitleTextBlockStyle` 20/600, `BodyTextBlockStyle` 14/400, `CaptionTextBlockStyle` 12/400) ; chemins en Cascadia Mono 13.

**Formes et espacements** : cartes rayon 8, contrôles rayon 4, boutons 36 px de haut (cible tactile ≥ 44 px pour les boutons icône) ; espacements par pas de 4 et 8 ; marges de page 40 px, 24 px sous 800 px de large. Ressources correspondantes dans `App.xaml` : `CardCornerRadius`, `PageMargin`, `PathFontFamily` et les styles `PageTitleTextBlockStyle`, `SecondaryTextBlockStyle`, `CardBorderStyle`.

## 4. Parcours utilisateur

Deux parcours portent l'essentiel de la valeur ; chacun se termine par une confirmation explicite et une session restaurable.

### 4.1 Nettoyer les restes orphelins (sans nom)

1. **Accueil** → « Analyser les restes ».
2. **Restes orphelins** : barre de progression (phase, dossier en cours, bouton Annuler), puis groupes triés par taille décroissante.
3. L'utilisateur déplie un groupe, lit les raisons, coche ou décoche ; le filtre de confiance réduit la liste.
4. « Mettre en quarantaine » → **Confirmation** : récapitulatif par type, total, rappel des 30 jours.
5. Fin : notification « 1,2 Go libérés » avec bouton « Annuler » (restauration immédiate) et lien vers l'**Historique**.

### 4.2 Désinstaller complètement une application

1. **Applications** : recherche, sélection d'une ligne, « Désinstaller complètement ».
2. **Assistant, étape 1** : nom, éditeur, taille, case « Créer un point de restauration » (cochée), bouton « Lancer la désinstallation ».
3. **Étape 2** : le désinstalleur officiel s'ouvre ; CleanSlate affiche « En attente du désinstalleur… » et ne permet pas de continuer avant sa fin.
4. **Étape 3** : résultat du désinstalleur + restes trouvés ; « Mettre les restes en quarantaine » ouvre la **Confirmation**.
5. **Étape 4** : résumé (application retirée, restes déplacés, espace libéré) et lien vers l'Historique.

Cas particuliers : si l'utilisateur annule le désinstalleur, l'étape 3 l'indique et ne propose que les restes déjà orphelins ; si aucun reste n'est trouvé, l'étape 3 est sautée.

Le parcours 4.2 compte 5 clics de l'écran Applications à la quarantaine (sélection, Désinstaller complètement, Lancer, Mettre en quarantaine, Confirmer).

## 5. États et textes d'interface

Chaque écran a un état vide, un état de chargement et un état d'erreur, écrits en phrases simples, au vouvoiement.

| Situation | Texte | Action proposée |
| --- | --- | --- |
| Aucun reste trouvé | « Rien à nettoyer : aucun reste d'application désinstallée. » | Retour à l'accueil |
| Analyse en cours | « Analyse de AppData\Roaming… (42 %) » | Annuler |
| Analyse annulée | « Analyse interrompue : résultats partiels affichés. » | Relancer |
| Fichier verrouillé | « 2 fichiers sont utilisés par un programme. Ils seront déplacés au prochain redémarrage. » | Voir les fichiers |
| Accès refusé | « Accès refusé à 1 élément. Il a été ignoré. » | Détails |
| Mode simulation | « Simulation : 6 éléments auraient été mis en quarantaine (1,2 Go). » | Désactiver la simulation |
| Historique vide | « Aucune session pour l'instant. » | Analyser les restes |
| Lancé sans droits admin | « CleanSlate a besoin des droits administrateur pour analyser Program Files et le registre machine. » | Redémarrer en administrateur |
| Désinstalleur introuvable | « Le programme de désinstallation de cette application est introuvable. » | Rechercher ses restes quand même |
| Confirmation renforcée (plus de 20 Go ou 50 000 fichiers) | « Vous allez mettre en quarantaine 23,4 Go (61 200 fichiers). Pour confirmer, tapez QUARANTAINE. » | Champ de saisie ; « Mettre en quarantaine » actif seulement quand le mot est exact |

Vocabulaire fixe : « mettre en quarantaine » (jamais « supprimer » tant que l'élément est restaurable), « restes », « confiance », « session ». Tailles en Ko / Mo / Go avec virgule décimale ; dates au format « 22 oct. 2026 ».

## 6. Accessibilité

L'application doit s'utiliser entièrement au clavier et avec le Narrateur ; vérification avec Accessibility Insights for Windows (ENF-08).

- Contraste du texte ≥ 4,5:1 dans les deux thèmes ; les jetons de la section 3 respectent ce seuil en clair.
- La confiance est toujours écrite en toutes lettres ; la couleur ne porte jamais seule une information.
- `AutomationProperties.Name` sur chaque bouton icône et chaque case à cocher d'élément (« Dossier C:\Users\…\audacity, 388 Mo, confiance élevée »).
- Ordre de tabulation : navigation, filtres, groupes, éléments, barre d'action ; `Espace` coche, `Entrée` déplie un groupe.
- Progression annoncée via une région `LiveSetting=Polite`, au plus toutes les 2 secondes.
- Prise en charge du mode Contraste élevé Windows (ressources `HighContrast` dans les `ThemeDictionaries`) et du zoom de texte jusqu'à 200 %.

## 7. Brief pour régénérer les maquettes

Ce brief décrit les planches de façon autonome. Il peut être donné tel quel à un outil de design (Canva ou autre) ou à un assistant IA pour produire de nouvelles versions des maquettes.

```text
Maquettes d'interface (16:9) pour « CleanSlate », utilitaire de bureau Windows 11 en français qui retrouve les fichiers laissés par des applications désinstallées et désinstalle complètement une application.
Fenêtre Windows 11 : barre de titre « CleanSlate », volet de navigation à gauche (Accueil, Applications, Restes orphelins, Historique, Réglages), contenu à droite. Fluent Design, sobre, coins arrondis 8 px, aucune photo, icônes linéaires.
Couleurs : accent #0F766E (survol #115E59, fond clair #E3F2F1), fond #F3F3F3, surfaces #FFFFFF, bordures #E5E5E5, texte #1A1A1A et #5C5C5C, danger #C42B1C réservé aux suppressions.
Badges de confiance avec le mot : Élevée (#0E700E sur #DFF6DD), Moyenne (#8A5100 sur #FFF4CE), Faible (#5C5C5C sur #EDEDED).
Police : Segoe UI Variable (Inter à défaut), chemins en police monospace.
Pages : 1 Charte graphique ; 2 Accueil ; 3 Applications ; 4 Restes orphelins ; 5 Assistant de désinstallation (étape 3 sur 4) ; 6 Dialogue de confirmation ; 7 Historique ; 8 Réglages.
Données d'exemple en français : GIMP 2.10.38 (412 Mo), Audacity, Discord, chemins C:\Users\Ana\AppData\…, 6 éléments sélectionnés pour 1,2 Go.
```

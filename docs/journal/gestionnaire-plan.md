# Plan — section « Gestionnaire » (D79)

Dernière mise à jour : 4 nov. 2026

Demande du porteur du 4 nov. 2026 : comprendre **et** libérer l'espace depuis CleanSlate, sur toute la machine, fichiers personnels compris, sans renvoyer vers les outils de Windows. Décision explicite du porteur : elle prime sur l'ancienne règle « ne jamais affaiblir la liste noire » (CLAUDE.md, règle 4) et sur l'arrêt obligatoire prévu dans ce cas.

## 1. Principes

- **Zones sensibles au lieu d'une liste noire bloquante.** La liste actuelle (`safety.rs` : `C:\Windows`, Program Files, ProgramData, pilotes, dossiers racines des profils, `HKLM\SYSTEM`, `SAM`, `SECURITY`, `HARDWARE`, `SOFTWARE\Microsoft`, `\Classes`, `Policies`…) devient la définition des **zones sensibles** :
  - les propositions automatiques (analyse des restes, catégories récupérables, « Tout libérer ») n'en proposent ni n'en précochent jamais un élément ;
  - une action choisie à la main dans une zone sensible exige le **mode expert** (désactivé par défaut, activé dans Réglages avec un avertissement) et la saisie de « CONFIRMER ».
- **Toujours refusé, même en mode expert** : un chemin relatif ou contenant `..`, un lien symbolique ou un point de jonction sur le chemin (règle 6), la racine d'un lecteur elle-même (supprimer `C:\`, c'est effacer un volume : les volumes restent en affichage seul, point A.11).
- **Deux modes pour chaque suppression**, choisis dans la confirmation : **Sûr** (Corbeille ou quarantaine, restaurable ; proposé par défaut) et **Immédiat** (définitif, le gain libéré tout de suite est affiché ; dans une zone sensible, en plus le mode expert et « CONFIRMER »).
- **Simulation par défaut** (`dry_run = true`) dans toutes les nouvelles API du moteur, de la FFI et de la CLI ; l'application passe `dry_run = false` explicitement après confirmation (D66).
- **Aucune action en arrière-plan.** À l'ouverture, une alerte si un lecteur a moins de 10 % de libre (seuil réglable).
- **Méthodes documentées seulement** pour les fichiers protégés (suppression au redémarrage, arrêt propre du service concerné) ; jamais de prise de propriété ni de changement de droits silencieux.
- **Tailles réelles sur disque** (fichiers compressés, OneDrive non téléchargés, liens physiques comptés une fois), gain mesuré avant / après.

## 2. Nouvelles exigences

| ID | Exigence |
| --- | --- |
| EF-22 | Zones sensibles : jamais proposées ni précochées automatiquement ; action manuelle seulement en mode expert avec « CONFIRMER ». |
| EF-23 | Modes « Sûr » (Corbeille ou quarantaine) et « Immédiat » (définitif) dans chaque confirmation ; « Sûr » par défaut ; aperçu : nombre, taille, zones sensibles, mode, gain libéré maintenant. |
| EF-24 | Quarantaine sur un autre lecteur, choisi dans Réglages, avec vérification de l'espace libre ; la confirmation dit « libéré maintenant » ou « libéré après purge ». |
| EF-25 | Caches régénérables (temporaires, navigateurs, miniatures, rapports d'erreurs et vidages, caches de paquets de développement) supprimés directement, jamais un fichier ouvert ; navigateur ouvert : proposition de le fermer. |
| EF-26 | Nettoyage système par les mécanismes officiels : DISM `StartComponentCleanup`, caches de Windows Update et de l'Optimisation de la distribution (arrêt et redémarrage propres des services), Windows.old, hibernation (`powercfg`), points de restauration (API système ; on garde le plus récent et ceux de moins de 30 jours, sauf mode expert). |
| EF-27 | Fichiers personnels : accès complet, jamais précochés ; Corbeille, déplacement vers un autre lecteur (espace vérifié, copie, comparaison SHA-256, puis suppression de l'original), suppression définitive, suppression sécurisée. Jamais les deux copies d'un doublon. |
| EF-28 | Fonctions de suppression : Corbeille, définitive, « supprimer et vider la Corbeille » ; vider les Corbeilles (taille par lecteur) ; purger la quarantaine (tout, session, élément) ; dossiers vides ; suppression au redémarrage (liste, annulation) ; suppression en masse par filtre avec aperçu ; suppression sécurisée (avertissement SSD, BitLocker recommandé). |
| EF-29 | Diagnostic : une phrase par lecteur, les 5 plus gros consommateurs expliqués, récupérable par catégorie (« sans risque » / « à vérifier »), « Libérer » par catégorie et « Tout libérer maintenant (sans risque) ». |
| EF-30 | Carte (la page Stockage), Explorateur trié par taille (taille, %, fichiers, date, recherche, sélection multiple, badge « zone sensible »), Catégories, Gros fichiers, Fichiers anciens, Doublons (taille puis SHA-256 via BCrypt, original proposé, actions sur les copies). |
| EF-31 | Système : hibernation, fichier d'échange, points de restauration, magasin de composants, Windows.old, caches de mise à jour, stockage réservé : taille, explication, conséquence, bouton qui l'exécute avec progression. |
| EF-32 | Suppressions en attente : redémarrage, Corbeilles, quarantaine, avec taille et bouton pour libérer. |
| EF-33 | Évolution : mesures enregistrées (SQLite) à chaque ouverture et analyse ; courbe de l'espace libre par lecteur, « ce qui a le plus grossi ». |
| EF-34 | Volumes : lecteurs et partitions (taille, utilisé, libre, système de fichiers, non alloué), affichage seul. |
| EF-35 | Partout : ouvrir dans l'Explorateur, propriétés, copier le chemin, désinstaller via l'assistant ; après chaque action, « X Go libérés maintenant » mesuré, entrée dans l'Historique, « Annuler » quand c'est possible ; export d'un rapport HTML et CSV sur le Bureau. |
| ENF-01 (réécrite) | Zones sensibles (liste ci-dessus) : jamais d'action automatique ; action manuelle en mode expert avec « CONFIRMER » ; refus absolu des chemins malformés, des liens et jonctions et des racines de lecteur. |
| ENF-02 (réécrite) | Chaque action a une confirmation explicite et un mode « Sûr » ou « Immédiat » ; simulation par défaut dans toutes les API. |
| ENF-13 | Analyses en lecture seule, parallèles, avec progression et annulation, sans suivre liens ni jonctions, accès refusés ignorés proprement ; analyse des restes < 60 s ; explorateur, doublons et nettoyage système en tâches séparées. |
| ENF-14 | Opérations système par API ou commandes officielles, code de retour vérifié, journalisation complète, message clair en cas d'échec ou de redémarrage nécessaire. |
| ENF-15 | Aucune donnée ne quitte la machine ; textes dans les `.resw` ; thèmes Clair, Sombre, Multicolore ; clavier et Narrateur. |

## 3. Étapes (un commit chacune, tests compris)

1. Documents : ce plan, D79, cahier (EF-22 à EF-35, ENF-01, ENF-02 réécrites, ENF-13 à ENF-15), spécifications, `CLAUDE.md` (règles 1, 4, 5 et 7), README.
2. Moteur : `zones` (zone sensible, autorisation manuelle, mode expert, « CONFIRMER ») ; les tests de liste noire deviennent des tests de zones (A.13).
3. Moteur : `manager::delete` (modes Sûr / Immédiat, Corbeille, quarantaine sur un autre lecteur avec espace vérifié, suppression au redémarrage, sécurisée, déplacement avec SHA-256) derrière des traits injectables qui refusent tout sur un moteur de test.
4. Moteur : analyses en lecture seule — diagnostic, catégories, gros fichiers, fichiers anciens, doublons, caches, Corbeilles, suppressions en attente, volumes — sur arborescences factices.
5. Moteur : nettoyage système (DISM, services de mise à jour, Windows.old, hibernation, points de restauration) derrière un trait `SystemCleaner` ; implémentation réelle jamais exécutée par les tests.
6. Moteur : mesures SQLite (Évolution), rapports HTML et CSV.
7. FFI et `EngineClient` : une fonction par analyse et par action, JSON, `dry_run` par défaut.
8. Application : page « Gestionnaire » et ses onglets (Diagnostic, Carte, Explorateur, Catégories, Gros fichiers, Fichiers anciens, Doublons, Système, Suppressions en attente, Évolution, Volumes), confirmation commune (mode, aperçu, zones sensibles, « CONFIRMER »), mode expert et seuil d'alerte dans Réglages, alerte à l'ouverture.
9. Captures de chaque onglet (`artifacts\captures\<date>\gestionnaire\`), scénarios Sandbox, audit de sécurité (`docs/securite/audit.md`), Release, installeur, CHANGELOG, feuille de route.

## 4. Scénarios Sandbox à ajouter

Caches ; nettoyage système ; points de restauration ; Corbeilles ; déplacement vers un autre lecteur ; suppression définitive ; suppression en masse ; suppression au redémarrage ; doublons ; quarantaine sur un autre lecteur ; mode expert sur une zone sensible avec restauration.

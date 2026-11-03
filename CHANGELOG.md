# Journal des modifications

Les changements visibles de CleanSlate. Le détail de chaque choix est dans
[`docs/decisions.md`](docs/decisions.md) (identifiants Dxx).

## [Non publié] — 3 nov. 2026

### Ajouté
- **Carte du stockage** (D73), dans la page Stockage :
  - une barre par disque (« 412 Go utilisés sur 512 Go — 100 Go libres ») ;
  - une carte en rectangles proportionnels, colorés par catégorie : Applications installées, Restes orphelins, Quarantaine de CleanSlate, Fichiers utilisateur, Windows et système, Autres ;
  - une légende avec le nom et la taille de chaque catégorie ;
  - trois niveaux de détail, avec un fil d'Ariane pour remonter ;
  - un clic sur un reste ouvre Restes orphelins, un clic sur une application ouvre Applications.
- Le moteur calcule la carte en lecture seule et en parallèle, avec une progression et un bouton Annuler. Il compte les tailles réelles sur le disque : les fichiers compressés comptent leur taille compressée, et les fichiers OneDrive qui ne sont pas téléchargés comptent 0 octet.
- La carte est gardée en mémoire jusqu'au prochain clic sur « Actualiser » ; l'heure du dernier calcul est affichée.
- Aperçu du stockage sur l'Accueil : le disque système et ses trois catégories les plus lourdes.
- Dans l'Historique, la place libre avant et après chaque action se lit dans la session concernée, sous un titre qui se déplie (D73).
- Restes orphelins : chaque groupe affiche son badge de meilleure confiance, même replié (D74).
- Mode de développement en simulation, pour les builds Debug seulement. Il sert aux captures d'écran de vérification (D74).

### Modifié
- Après un nettoyage, la page Stockage n'affiche plus le détail du calcul. Elle montre une seule ligne, par exemple « + 1,2 Go libérés par le dernier nettoyage », et la carte se met à jour sans relancer le calcul (D73).
- Le thème **Multicolore** a été revu :
  - le fond de la fenêtre et les en-têtes de page sont colorés ;
  - les cartes sont de couleurs différentes ;
  - chaque icône du menu a sa propre couleur ;
  - les barres de progression sont en arc-en-ciel.

  Le choix **Système** est de retour dans Réglages (D72).

### Corrigé
- Les badges de confiance « Élevée », « Moyenne » et « Faible » sont visibles dans tous les thèmes (D72).
- Revenir à une page déjà sélectionnée dans le menu, par exemple depuis l'assistant, l'ouvre bien (D74).

## 0.1.0 — 31 oct. 2026

Première version installée sur le PC du porteur (étape A4) : détection des restes, désinstallation complète, quarantaine restaurable, Historique, Réglages, premier lancement et page Stockage (D01 à D70).

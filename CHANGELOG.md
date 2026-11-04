# Journal des modifications

Les changements visibles de CleanSlate. Le détail de chaque choix est dans
[`docs/decisions.md`](docs/decisions.md) (identifiants Dxx).

## 0.1.0 bêta 1 — 5 nov. 2026

Première version de test, publiée sur GitHub : https://github.com/JOHNNY-nick/CleanSlate/releases/tag/v0.1.0-beta.1
(l'état ci-dessous est celui de cette version ; le détail de chaque étape est dans les décisions D76 à D86).

### Ajouté
- **Gestionnaire** (D79, D82, D86) : comprendre et libérer l'espace de toute la machine, fichiers personnels compris.
  - 8 onglets : Diagnostic, Catégories, Gros fichiers, Fichiers anciens, Doublons, Système, Suppressions en attente, Volumes.
  - Le Diagnostic donne une phrase par disque, les 5 plus gros consommateurs, et ce qui peut être libéré, « sans risque » ou « à vérifier », avec « Tout libérer maintenant (sans risque) » et un lien vers la carte du stockage.
  - En bas : « Supprimer… » (mode « Sûr », Corbeille ou quarantaine de CleanSlate pour un autre disque, ou « Immédiat », définitif), « Ouvrir dans l'Explorateur », « Exporter le rapport » et « ? » qui explique chaque action.
  - Système : nettoyage par les outils officiels de Windows (« Nettoyer… ») ; Suppressions en attente : annuler une suppression prévue au redémarrage ; Volumes : disques physiques et espace non alloué.
  - « Exporter le rapport » (D85) : un PDF dans le style du livre (couverture, diagnostic, ce qui peut être libéré, catégories, plus gros fichiers, fichiers anciens) et le tableau CSV, sur le Bureau ou dans le dossier d'export.
  - Les actions apparaissent dans l'Historique avec l'espace libéré ; alerte d'espace faible à l'ouverture.
- **Stockage** (D81, D83) : la carte se parcourt sans limite de profondeur, et sous la carte, la liste du dossier affiché (dossiers et fichiers, taille, date, type), liée à la carte ; on coche puis on supprime avec la même confirmation. Les dossiers des applications se parcourent aussi, avec un lien vers Applications.
- Zones sensibles et mode expert (D79) : les zones de Windows et des programmes ne sont jamais proposées automatiquement ; y agir à la main demande le mode expert et « CONFIRMER ».
- Un indicateur en bas du menu montre l'opération longue en cours sur toutes les pages et y ramène d'un clic (D82).
- Barre de progression pendant la mise en quarantaine des restes (D77).
- Critères de confiance plus pointus (D78) : raccourcis qui fonctionnent, Windows Installer, App Paths et associations, gestionnaires de paquets, lancements récents ; leur calcul est expliqué par le « ? » de Restes orphelins.

### Modifié
- Thème **Multicolore** au style « street » (D76) : un mur couvert de peinture vive, du texte toujours sur des autocollants à contour épais et ombre décalée, des lignes de liste à contour coloré, une carte du stockage en dégradés ; trois styles (Éclaboussures, Aquarelle, Explosion) et un fond sombre au choix dans Réglages. La section active du menu est marquée d'un autocollant (deux autres variantes à l'essai), le bouton du menu est dans une bulle (D82).
- Les explications de Stockage et de Restes orphelins sont dans un bouton « ? » (D86).
- Une analyse continue quand on change de page, et on la retrouve au retour ; une demande faite pendant une opération attend son tour (D82).

### Corrigé
- Plus aucune fenêtre de console : les disques sont lus par une fonction de Windows, et les outils de Windows se lancent sans fenêtre (D82).
- Plus de « Busy » : ni dans le Gestionnaire ni au clic sur la carte ; tous les messages d'erreur sont en français (D82).
- Plus rapide (D83, D84) : un niveau de la carte ne relance plus une analyse complète des restes (plusieurs minutes sur VirtualBox), la liste des applications n'est plus relue à chaque page, chaque niveau de la carte se dessine en une fois, le Gestionnaire ne relit pas tout à chaque visite.
- Texte de la carte net : les étiquettes ne sont plus inclinées (D86).
- Annuler une suppression au redémarrage ne retirait rien quand Windows avait d'autres opérations en attente (D80) ; le cache de Windows Update absent n'est plus une erreur ; en mode « Sûr », un fichier d'un autre disque va bien dans la quarantaine.
- Après une action du Gestionnaire, l'espace libéré reste affiché (D84) ; sans analyse, « Exporter le rapport » dit quoi faire (D85).
- Historique : une session purgée dit « supprimée définitivement, plus restaurable » (D76).

## 3 nov. 2026

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
  - les barres de progression sont en arc-en-ciel ;
  - des éclaboussures de peinture vives aux cinq couleurs apparaissent dans le menu, à droite des en-têtes et autour du contenu, d'après les références du porteur ; aucun texte n'est posé dessus (D75).

  Le choix **Système** est de retour dans Réglages (D72).

### Corrigé
- Les badges de confiance « Élevée », « Moyenne » et « Faible » sont visibles dans tous les thèmes (D72).
- Revenir à une page déjà sélectionnée dans le menu, par exemple depuis l'assistant, l'ouvre bien (D74).

## 0.1.0 — 31 oct. 2026

Première version installée sur le PC du porteur (étape A4) : détection des restes, désinstallation complète, quarantaine restaurable, Historique, Réglages, premier lancement et page Stockage (D01 à D70).

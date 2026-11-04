# Journal des modifications

Les changements visibles de CleanSlate. Le détail de chaque choix est dans
[`docs/decisions.md`](docs/decisions.md) (identifiants Dxx).

## [Non publié] — 4 nov. 2026

### Ajouté
- **Gestionnaire** (D79) : comprendre et libérer l'espace de toute la machine, fichiers personnels compris.
  - Onglets : Diagnostic, Carte, Explorateur, Catégories, Gros fichiers, Fichiers anciens, Doublons, Système, Suppressions en attente, Évolution, Volumes.
  - Le Diagnostic donne une phrase par disque, les 5 plus gros consommateurs, et ce qui peut être libéré, « sans risque » ou « à vérifier », avec « Tout libérer maintenant (sans risque) ».
  - Chaque suppression se confirme, en mode « Sûr » (Corbeille) ou « Immédiat » (définitif).
  - Autres actions : suppression au redémarrage, suppression sécurisée, déplacement vérifié vers un autre disque, nettoyage des caches et des Corbeilles, nettoyage système par les outils officiels de Windows.
  - Rapport HTML et CSV, alerte d'espace faible à l'ouverture.
  - Une suppression prévue au redémarrage peut être annulée ; en mode « Sûr », un fichier d'un autre disque va dans la quarantaine de CleanSlate (l'espace est libéré tout de suite, restaurable 30 jours).
  - Les actions du Gestionnaire apparaissent dans l'Historique avec l'espace libéré ; l'onglet Volumes montre les disques physiques et leur espace non alloué.
  - Scénario Windows Sandbox « Gestionnaire » (`pwsh scripts/sandbox/prepare.ps1 -Scenario Gestionnaire -Launch`).
- Zones sensibles et mode expert (D79) : les zones de Windows et des programmes ne sont jamais proposées automatiquement ; y agir à la main demande le mode expert et « CONFIRMER ».
- Barre de progression pendant la mise en quarantaine des restes (D77).
- Critères de confiance plus pointus (D78) : raccourcis qui fonctionnent, Windows Installer, App Paths et associations, gestionnaires de paquets, lancements récents. Leur calcul est expliqué dans Restes orphelins.

### Modifié
- Stockage (D83) : sous la carte, la liste du dossier affiché (dossiers et fichiers, taille, date, type), liée à la carte : un clic sur un dossier l'ouvre des deux côtés ; on coche puis on supprime avec la confirmation habituelle. Les dossiers des applications (Steam, Microsoft, Adobe…) se parcourent comme les autres, avec un lien vers Applications.
- Gestionnaire (D83) : l'onglet « Explorateur » s'appelle « Recherche » et dit ce qu'il fait.
- Gestionnaire (D82) : onglet « Carte » retiré (la carte est la page Stockage) ; « Analyse en cours… (Annuler) » ; infobulle de chaque action et bouton « ? ».
- Carte (D82) : double-clic sur un dossier qui ne contient que des fichiers, ou clic droit « Voir les fichiers » sur tout dossier : la liste de ses fichiers dans l'Explorateur du Gestionnaire, et « Revenir à la carte » au même endroit.
- Menu (D82) : la section active marquée dans le style street (trois variantes à choisir), le bouton du menu dans une bulle.
- Carte du stockage (D81) : on peut creuser sans limite (un dossier pas encore détaillé est calculé au clic) ; un clic sur un dossier qui ne contient que des fichiers, ou sur un fichier, ouvre le Gestionnaire avec ces fichiers pour choisir quoi supprimer ; le nom des petits rectangles s'affiche.
- Thème **Multicolore** au style « street » de la maquette de la carte du stockage (D76) :
  - un mur couvert de peinture vive (grandes éclaboussures, aquarelle, traînées, coulures, gouttes, ville) ;
  - du texte toujours sur des autocollants à contour épais et ombre décalée ;
  - des titres inclinés sur une éclaboussure, le menu autocollant avec l'élément actif en vert ;
  - des lignes de liste à contour coloré et des pastilles d'état ;
  - une carte du stockage en dégradés.

  Trois styles (Éclaboussures, Aquarelle, Explosion) et un fond sombre au choix dans Réglages.

### Corrigé
- Plus rapide (D84) : la liste des applications n'est plus relue à chaque page ni à chaque niveau de la carte ; chaque niveau de la carte se dessine en une fois ; le Gestionnaire ne relit pas tout à chaque visite. Après une action du Gestionnaire, l'espace libéré reste affiché.
- Carte : un dossier comme VirtualBox ne fait plus attendre plusieurs minutes (une analyse complète des restes était relancée avant chaque niveau) ; le bouton du menu n'a plus d'ombre décalée en Clair et Sombre (D83).
- Plus aucune fenêtre de console : les disques sont lus par une fonction de Windows, et les outils de Windows se lancent sans fenêtre (D82).
- Gestionnaire : une analyse continue quand on change de page, et on la retrouve au retour, avec sa progression ; une demande faite pendant une analyse attend son tour au lieu d'afficher « Busy ». Un indicateur en bas du menu montre l'opération en cours sur toutes les pages. Tous les messages d'erreur sont en français (D82).
- Carte du stockage : plus de « Busy » au clic sur un dossier ; un niveau encore à calculer affiche un rond de chargement (D82).
- Gestionnaire : annuler une suppression au redémarrage ne retirait rien quand Windows avait d'autres opérations en attente, et aurait pu décaler la liste (D80) ; le nettoyage du cache de Windows Update n'échoue plus quand ce cache n'existe pas. En mode « Sûr », un fichier d'un autre disque va bien dans la quarantaine (l'espace libre du disque de la quarantaine était mal lu).
- Historique : une session purgée n'affiche plus une date de purge à venir ; elle dit « supprimée définitivement, plus restaurable » (D76).

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

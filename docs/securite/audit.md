# Audit de sécurité — Gestionnaire et zones sensibles (D79)

Dernière mise à jour : 4 nov. 2026

Portée : les changements de D79 (fin de la liste noire bloquante, zones sensibles, mode expert, mode « Immédiat », opérations système) et les nouvelles surfaces d'attaque. L'audit complet de l'étape B1 de la feuille de route reste à faire.

## 1. Ce qui change

| Avant (D01 à D78) | Après (D79) |
| --- | --- |
| La liste noire de `safety.rs` bloque toute action. | Elle définit les **zones sensibles** : les propositions automatiques l'appliquent toujours ; une action manuelle y passe par `zones::authorize_path` / `authorize_key`, qui exige le mode expert puis « CONFIRMER ». |
| Tout passe par la quarantaine (sauf D66). | Mode « Sûr » (Corbeille, restaurable) par défaut ; mode « Immédiat » (définitif) choisi dans la confirmation. Caches régénérables supprimés directement. |
| Aucune opération système. | Nettoyage système par DISM, `sc`, la cmdlet de l'Optimisation de la distribution, `cleanmgr`, `powercfg`, `vssadmin`. |

## 2. Garde-fous conservés

- **Refus absolus, même en mode expert** : chemin relatif ou avec `.` / `..`, chemin réseau, racine d'un lecteur ou d'une ruche, lien symbolique ou jonction sur le chemin (vérifié juste avant d'agir), chemin canonique différent du chemin donné. Tests : `zones::tests::roots_malformed_paths_and_links_are_always_refused`.
- **L'autorisation précède chaque action, élément par élément** (`manager::delete`, `delete_at_restart`, `secure_delete`, `move_to`, `clean_caches`). Tests : `deletion_respects_the_zones_the_mode_and_the_simulation`, `manager_analyses_and_simulates_through_the_ffi`.
- **Simulation par défaut** dans toutes les requêtes du Gestionnaire (`dry_run: true`) ; le moteur de test refuse la Corbeille, le redémarrage, les volumes et le nettoyage système (`NoSystemShell`, `NoSystemCleaner`).
- **Jamais de changement de propriétaire ni de droits** ; fichiers verrouillés : suppression au redémarrage (`MoveFileExW`, documenté) seulement.
- **Doublons** : la copie à garder n'est jamais dans la liste des copies à supprimer.
- **Déplacement** : espace vérifié, copie, SHA-256 comparé fichier par fichier ; l'original n'est supprimé que si toutes les copies sont identiques.
- **Commandes système** lancées par leur chemin dans `System32`, sans interpréteur de commandes (règle 8), code de retour vérifié et journalisé ; 3010 = redémarrage nécessaire.
- **Application** : un élément d'une zone sensible n'est jamais précoché ; hors mode expert, l'action est refusée avant même la confirmation ; en mode expert, la confirmation explique le risque et n'active le bouton qu'une fois « CONFIRMER » tapé.

## 3. Nouvelles surfaces d'attaque et risque résiduel

| Menace | Mesure | Risque résiduel |
| --- | --- | --- |
| L'utilisateur supprime par erreur un fichier de Windows. | Zones sensibles, mode expert désactivé par défaut, « CONFIRMER », mode « Sûr » par défaut. | **Accepté par le porteur** : en mode expert, un utilisateur décidé peut casser Windows. |
| Course TOCTOU : un dossier normal remplacé par une jonction entre l'aperçu et l'action. | Vérification des liens juste avant d'agir ; `remove_dir_all` ne suit pas les liens internes. | Faible : une fenêtre de quelques millisecondes subsiste entre la vérification et l'appel système (comme pour la quarantaine). |
| Un programme sans droits pose un fichier piège dans un cache de l'utilisateur. | Les caches ne suppriment que leur contenu, sans suivre de lien ; un fichier ouvert est ignoré. | Faible : au pire, un fichier du cache est supprimé. |
| Exception étroite D79 : les caches de l'utilisateur (dont `%LOCALAPPDATA%\Temp`) ne sont pas une zone sensible. | Liste fermée de dossiers exacts (`manager::is_user_cache`) ; `C:\Windows\Temp` reste sensible. | Faible. |
| Détournement des outils système (DISM, `vssadmin`…) | Chemins absolus dans `%WINDIR%\System32`, arguments fixes. | Faible : suppose déjà un compte administrateur compromis. |
| Suppression sécurisée sur SSD | Avertissement : l'usure répartie et TRIM empêchent de garantir l'effacement ; BitLocker recommandé. | Assumé et dit à l'utilisateur. |
| Nettoyage de Windows Update pendant une installation | Services arrêtés puis redémarrés, même en cas d'échec du vidage. | Faible : Windows retélécharge ce qui manque. |

## 4. Limites connues

- Restaurer depuis la Corbeille se fait par l'Explorateur de Windows (CleanSlate ne restaure que sa quarantaine).
- Le Sandbox n'a qu'un disque : le scénario y attache un disque virtuel (VHDX) comme second volume. Un lecteur `subst` est refusé comme source (alias : le chemin ne se résout pas en lui-même, règle 6), mais accepté comme destination d'un déplacement.
- Scénario Sandbox « Gestionnaire » : 15 / 15 le 4 nov. 2026 (run `gestionnaire-20261104-114641`).

namespace CleanSlate.Interop;

/// <summary>
/// French text of every error code of the engine (D82): the interface never shows a technical
/// message in English. The code and the engine's own message stay in
/// <see cref="EngineException.Detail"/> for the logs.
/// </summary>
public static class EngineErrors
{
    // Codes of CoreError::code (error.rs), of the FFI (lib.rs) and of EngineClient.
    private static readonly Dictionary<string, string> Texts = new()
    {
        ["NOT_FOUND"] = "Introuvable{0} : l'élément a peut-être déjà été supprimé ou déplacé.",
        ["ACCESS_DENIED"] = "Accès refusé{0} : Windows ou un programme protège cet élément.",
        ["IO_ERROR"] = "Lecture ou écriture impossible{0} : l'élément est peut-être ouvert dans un programme.",
        ["REGISTRY_ERROR"] = "Le registre de Windows a refusé l'opération{0}.",
        ["INVALID_INPUT"] = "Demande invalide : CleanSlate n'a rien modifié.",
        ["PROTECTED"] = "Élément protégé{0} : CleanSlate ne touche jamais aux liens, aux jonctions ni aux racines de lecteur.",
        ["CONFIRMATION_REQUIRED"] = "Confirmation nécessaire : tapez « CONFIRMER » pour continuer.",
        ["EXPERT_REQUIRED"] = "Zone sensible{0} : activez le mode expert dans Réglages pour agir ici.",
        ["NO_UNINSTALLER"] = "Cette application n'a pas de désinstalleur officiel.",
        ["UNSAFE_UNINSTALLER"] = "Le désinstalleur de cette application a été refusé : sa commande n'est pas sûre.",
        ["BUSY"] = "Une analyse est déjà en cours. Attendez sa fin ou annulez-la.",
        ["CANCELLED"] = "Opération annulée.",
        ["TAMPERED"] = "Cette session de la quarantaine n'a pas été écrite par CleanSlate sur ce PC : elle reste affichée, mais n'est pas restaurée.",
        ["INTEGRITY_UNAVAILABLE"] = "CleanSlate ne peut pas vérifier la quarantaine sans les droits d'administrateur : rien n'a été restauré.",
        ["INTERNAL_PANIC"] = "Erreur interne du moteur de CleanSlate : l'opération s'est arrêtée. Le journal contient le détail.",
        ["SERIALIZATION_ERROR"] = "Réponse illisible du moteur de CleanSlate. Le journal contient le détail.",
        ["INVALID_RESPONSE"] = "Réponse illisible du moteur de CleanSlate. Le journal contient le détail.",
        ["NULL_RESPONSE"] = "Le moteur de CleanSlate n'a pas répondu.",
        ["ENGINE_CREATE_FAILED"] = "Le moteur de CleanSlate n'a pas pu démarrer.",
    };

    /// <summary>True when <paramref name="code"/> has its own French text.</summary>
    public static bool IsKnown(string code) => Texts.ContainsKey(code);

    /// <summary>
    /// French text of <paramref name="code"/>, with the path concerned when there is one; an
    /// unknown code gets a general sentence, never the English message.
    /// </summary>
    public static string Describe(string code, string? path = null)
    {
        string where = string.IsNullOrEmpty(path) ? string.Empty : $" ({path})";
        return Texts.TryGetValue(code, out string? text)
            ? string.Format(System.Globalization.CultureInfo.GetCultureInfo("fr-FR"), text, where)
            : "Erreur inattendue du moteur de CleanSlate. Le journal contient le détail.";
    }
}

using System.Collections.Generic;

namespace Nekobox;

/// <summary>
/// キャラクターの感情文字列を絵文字にマッピングするユーティリティ。
/// </summary>
public static class EmotionHelper
{
    private static readonly Dictionary<string, string> EmojiMap = new()
    {
        { "楽しい",   "😄" },
        { "嬉しい",   "😊" },
        { "普通",     "😐" },
        { "悲しい",   "😢" },
        { "イライラ", "😠" },
        { "うんざり", "😩" },
        { "びっくり", "😲" },
        { "怖い",     "😨" },
    };

    /// <summary>感情文字列に対応する絵文字を返す。不明な場合は "😐"。</summary>
    public static string ToEmoji(string emotion) =>
        EmojiMap.GetValueOrDefault(emotion, "😐");
}

using Godot;

namespace Nekobox;

/// <summary>
/// 画面下部の半透明会話チャットウィンドウ。
/// キャラクターからのメッセージとエラーを表示する。
/// </summary>
public partial class ChatWindow : PanelContainer
{
    private VBoxContainer   _messagesList    = null!;
    private ScrollContainer _scrollContainer = null!;
    private bool            _needsScroll;

    public override void _Ready()
    {
        _scrollContainer = GetNode<ScrollContainer>("ScrollContainer");
        _messagesList    = GetNode<VBoxContainer>("ScrollContainer/MessagesList");
    }

    // レイアウト確定後（次フレーム以降）にスクロールを実行
    public override void _Process(double delta)
    {
        if (!_needsScroll) return;
        _needsScroll = false;
        _scrollContainer.ScrollVertical = int.MaxValue;
    }

    // ──────────────────────────────────────────────────────────
    //  Public API
    // ──────────────────────────────────────────────────────────

    /// <summary>ユーザー発言とキャラクター返答を1ペアで追加する。</summary>
    public void AddMessage(
        string userName, string userMessage,
        string characterName, string characterMessage, string emotion)
    {
        var emoji = EmotionHelper.ToEmoji(emotion);
        // ユーザー行を灰色、キャラ行を通常色で表示
        var text = $"[color=#999999]{userName}: {userMessage}[/color]" +
                   $"\n{emoji} {characterName}: {characterMessage}";
        AddLabel(text, isRich: true);
    }

    /// <summary>すべてのメッセージを消去する。</summary>
    public void Clear()
    {
        foreach (var child in _messagesList.GetChildren())
            child.QueueFree();
        _needsScroll = false;
    }

    /// <summary>エラーメッセージを赤テキストで追加する。</summary>
    public void AddError(string errorId, string errorMessage)
    {
        AddLabel($"[color=red]⚠ エラー [{errorId}]:\n{errorMessage}[/color]", isRich: true);
    }

    // ──────────────────────────────────────────────────────────
    //  内部ユーティリティ
    // ──────────────────────────────────────────────────────────

    private void AddLabel(string text, bool isRich = false)
    {
        if (isRich)
        {
            var lbl = new RichTextLabel
            {
                BbcodeEnabled  = true,
                AutowrapMode   = TextServer.AutowrapMode.Word,
                FitContent     = true,
                Text           = text,
            };
            _messagesList.AddChild(lbl);
        }
        else
        {
            var lbl = new Label
            {
                AutowrapMode = TextServer.AutowrapMode.Word,
                Text         = text,
            };
            _messagesList.AddChild(lbl);
        }

        // 次の _Process でレイアウト確定後にスクロール
        _needsScroll = true;
    }
}

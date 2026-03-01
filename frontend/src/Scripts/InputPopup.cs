using System;
using Godot;

namespace Nekobox;

/// <summary>
/// ユーザーのテキスト入力を受け付けるモードレスポップアップウィンドウ。
/// Enter: 送信 / Shift+Enter: 改行
/// </summary>
public partial class InputPopup : Window
{
    private TextEdit _inputText  = null!;
    private Button   _sendButton = null!;

    /// <summary>メッセージ送信時に呼ばれるコールバック。</summary>
    public Action<string>? OnMessageSubmitted { get; set; }

    public override void _Ready()
    {
        _inputText  = GetNode<TextEdit>("VBoxContainer/InputText");
        _sendButton = GetNode<Button>("VBoxContainer/SendButton");

        // モードレス設定
        Exclusive = false;

        _sendButton.Pressed       += OnSendPressed;
        _inputText.GuiInput       += OnInputGuiInput;

        // ウィンドウを閉じようとしたときも非表示にするだけ（終了しない）
        CloseRequested += () => Hide();
    }

    // ──────────────────────────────────────────────────────────
    //  イベントハンドラ
    // ──────────────────────────────────────────────────────────

    private void OnSendPressed() => SubmitMessage();

    private void OnInputGuiInput(InputEvent @event)
    {
        if (@event is not InputEventKey { Pressed: true } key) return;

        // Enter のみ → 送信（Shift+Enter は標準の改行に任せる）
        if (key.Keycode == Key.Enter && !key.ShiftPressed)
        {
            GetViewport().SetInputAsHandled();
            SubmitMessage();
        }
    }

    // ──────────────────────────────────────────────────────────
    //  内部ロジック
    // ──────────────────────────────────────────────────────────

    private void SubmitMessage()
    {
        var text = _inputText.Text.Trim();
        if (string.IsNullOrEmpty(text)) return;

        OnMessageSubmitted?.Invoke(text);
        _inputText.Clear();
        _inputText.GrabFocus();
    }
}

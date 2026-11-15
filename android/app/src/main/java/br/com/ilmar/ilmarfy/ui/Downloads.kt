package br.com.ilmar.ilmarfy.ui

import br.com.ilmar.ilmarfy.core.Core
import br.com.ilmar.ilmarfy.core.friendly
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import uniffi.ilmarfy_ffi.DownloadListener

/** Progresso de um download em andamento (de uma faixa ou de uma playlist) */
data class DownloadProgress(val label: String, val fraction: Float?)

/**
 * Downloads para ouvir offline. Rodam no escopo do app, então continuam ao trocar de tela.
 * A chave é "track:<id>" ou "playlist:<id>".
 */
object Downloads {
    private val _active = MutableStateFlow<Map<String, DownloadProgress>>(emptyMap())
    val active: StateFlow<Map<String, DownloadProgress>> = _active.asStateFlow()

    private val _messages = MutableStateFlow<String?>(null)
    val messages: StateFlow<String?> = _messages.asStateFlow()

    fun consumeMessage() {
        _messages.value = null
    }

    private fun set(key: String, p: DownloadProgress?) = _active.update { if (p == null) it - key else it + (key to p) }

    fun downloadTrack(id: String, title: String) {
        val key = "track:$id"
        if (key in _active.value) return
        set(key, DownloadProgress(title, 0f))
        Core.appScope.launch {
            val listener = object : DownloadListener {
                override fun onProgress(doneBytes: ULong, totalBytes: ULong): Boolean {
                    val f = if (totalBytes > 0u) doneBytes.toFloat() / totalBytes.toFloat() else null
                    set(key, DownloadProgress(title, f))
                    return key in _active.value
                }

                override fun onTrack(index: UInt, total: UInt, title: String) = true
            }
            runCatching { Core.get().download(id, listener) }
                .onSuccess { _messages.value = "\"$title\" disponível offline" }
                .onFailure { _messages.value = "Falha ao baixar: ${it.friendly()}" }
            set(key, null)
            Core.changed()
        }
    }

    fun downloadPlaylist(id: String, name: String) {
        val key = "playlist:$id"
        if (key in _active.value) return
        set(key, DownloadProgress("Preparando…", null))
        Core.appScope.launch {
            val listener = object : DownloadListener {
                override fun onProgress(doneBytes: ULong, totalBytes: ULong) = key in _active.value

                override fun onTrack(index: UInt, total: UInt, title: String): Boolean {
                    set(key, DownloadProgress("${index + 1u} de $total • $title", (index.toFloat()) / total.toFloat()))
                    return key in _active.value
                }
            }
            runCatching { Core.get().downloadPlaylist(id, listener) }
                .onSuccess { r ->
                    _messages.value = buildString {
                        append("\"$name\": ${r.downloaded} faixas baixadas")
                        if (r.failures.isNotEmpty()) append(", ${r.failures.size} falharam")
                    }
                }
                .onFailure { _messages.value = "Falha ao baixar: ${it.friendly()}" }
            set(key, null)
            Core.changed()
        }
    }

    fun cancel(key: String) = set(key, null)

    fun removeTrack(id: String) {
        Core.appScope.launch {
            runCatching { Core.get().removeDownload(id) }
            Core.changed()
        }
    }
}

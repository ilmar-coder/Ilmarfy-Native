package br.com.ilmar.ilmarfy.core

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.core.content.FileProvider
import br.com.ilmar.ilmarfy.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/** Versão publicada no servidor (data/apps/ilmarfy-android/latest.json, gravado pelo release.sh) */
data class Release(val versionCode: Int, val versionName: String, val notes: String, val size: Long, val sha256: String)

sealed interface UpdateState {
    data object Idle : UpdateState
    data class Available(val release: Release) : UpdateState
    data class Downloading(val release: Release, val fraction: Float) : UpdateState
    data class Failed(val message: String) : UpdateState
}

/**
 * Atualização do app pelo próprio app (sem Play Store): consulta o servidor, baixa o APK com o token
 * do aparelho, confere o SHA-256 e abre o instalador do Android.
 */
object Updater {
    private const val APP = "ilmarfy-android"
    private val _state = MutableStateFlow<UpdateState>(UpdateState.Idle)
    val state: StateFlow<UpdateState> = _state.asStateFlow()

    val currentVersion: String get() = "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})"

    private fun open(path: String): HttpURLConnection =
        (URL(Core.server + path).openConnection() as HttpURLConnection).apply {
            Core.get().authHeaderValue()?.let { setRequestProperty("Authorization", it) }
            connectTimeout = 15_000
            readTimeout = 60_000
        }

    /** Consulta o servidor; devolve a versão nova (ou null se já está na mais recente) */
    suspend fun check(): Release? = withContext(Dispatchers.IO) {
        val conn = open("/api/apps/$APP/latest")
        try {
            if (conn.responseCode == 404) return@withContext null
            if (conn.responseCode != 200) error("Servidor respondeu ${conn.responseCode}")
            val j = JSONObject(conn.inputStream.bufferedReader().readText())
            val rel = Release(j.getInt("version_code"), j.getString("version_name"), j.optString("notes"), j.optLong("size"), j.optString("sha256"))
            if (rel.versionCode > BuildConfig.VERSION_CODE) {
                _state.value = UpdateState.Available(rel)
                rel
            } else {
                if (_state.value is UpdateState.Available) _state.value = UpdateState.Idle
                null
            }
        } finally {
            conn.disconnect()
        }
    }

    fun dismiss() {
        _state.value = UpdateState.Idle
    }

    /** Baixa e abre o instalador. Continua mesmo saindo da tela. */
    fun downloadAndInstall(context: Context, rel: Release) {
        val app = context.applicationContext
        if (_state.value is UpdateState.Downloading) return
        _state.value = UpdateState.Downloading(rel, 0f)
        Core.appScope.launch {
            runCatching {
                val dir = File(app.cacheDir, "updates").apply { mkdirs() }
                dir.listFiles()?.forEach { it.delete() }
                val file = File(dir, "ilmarfy-${rel.versionName}.apk")
                val conn = open("/api/apps/$APP/download")
                try {
                    if (conn.responseCode != 200) error("Servidor respondeu ${conn.responseCode}")
                    val total = conn.contentLengthLong.takeIf { it > 0 } ?: rel.size
                    val digest = MessageDigest.getInstance("SHA-256")
                    conn.inputStream.use { input ->
                        file.outputStream().use { out ->
                            val buf = ByteArray(64 * 1024)
                            var done = 0L
                            while (true) {
                                val n = input.read(buf)
                                if (n < 0) break
                                out.write(buf, 0, n)
                                digest.update(buf, 0, n)
                                done += n
                                if (total > 0) _state.value = UpdateState.Downloading(rel, done.toFloat() / total)
                            }
                        }
                    }
                    val sha = digest.digest().joinToString("") { "%02x".format(it) }
                    if (rel.sha256.isNotBlank() && !sha.equals(rel.sha256, ignoreCase = true)) {
                        file.delete()
                        error("O arquivo baixado está corrompido; tente de novo")
                    }
                } finally {
                    conn.disconnect()
                }
                file
            }.onSuccess { file ->
                _state.value = UpdateState.Available(rel)
                install(app, file)
            }.onFailure {
                _state.value = UpdateState.Failed(it.friendly())
            }
        }
    }

    private fun install(context: Context, file: File) {
        // Primeira vez: o Android pede para permitir que o Ilmarfy instale atualizações
        if (!context.packageManager.canRequestPackageInstalls()) {
            context.startActivity(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
            _state.value = UpdateState.Failed("Permita \"Instalar apps desconhecidos\" para o Ilmarfy e toque em Atualizar de novo")
            return
        }
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.files", file)
        context.startActivity(
            Intent(Intent.ACTION_VIEW)
                .setDataAndType(uri, "application/vnd.android.package-archive")
                .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK),
        )
    }
}

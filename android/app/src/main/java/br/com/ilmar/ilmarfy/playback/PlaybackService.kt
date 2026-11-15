package br.com.ilmar.ilmarfy.playback

import android.app.PendingIntent
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.Player
import androidx.media3.datasource.DefaultDataSource
import androidx.media3.datasource.DefaultHttpDataSource
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import br.com.ilmar.ilmarfy.MainActivity
import br.com.ilmar.ilmarfy.core.Core
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import java.io.File
import kotlinx.coroutines.launch

/**
 * Serviço de reprodução: toca em segundo plano, com notificação, tela de bloqueio e botões do fone.
 * Os itens chegam só com o id ("track:<id>" ou "flow:<videoId>"); aqui decidimos de onde tocar.
 */
class PlaybackService : MediaSessionService() {
    private var session: MediaSession? = null
    private lateinit var tracker: PlayTracker

    override fun onCreate() {
        super.onCreate()

        val http = DefaultHttpDataSource.Factory()
            .setUserAgent("Ilmarfy-Android")
            .setAllowCrossProtocolRedirects(true)
        // Acrescenta o token em toda requisição ao servidor (streaming das faixas e prévias do Flow)
        val dataSource = ResolvingDataSource.Factory(DefaultDataSource.Factory(this, http)) { spec ->
            val header = Core.get().authHeaderValue()
            if (header != null && spec.uri.toString().startsWith(Core.server)) {
                spec.withAdditionalHeaders(mapOf<String, String>("Authorization" to header))
            } else {
                spec
            }
        }

        val player = ExoPlayer.Builder(this)
            .setMediaSourceFactory(DefaultMediaSourceFactory(dataSource))
            .setAudioAttributes(
                AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MUSIC).build(),
                /* handleAudioFocus = */ true,
            )
            .setHandleAudioBecomingNoisy(true) // pausa ao tirar o fone
            .setWakeMode(C.WAKE_MODE_NETWORK)
            .build()

        tracker = PlayTracker(player)
        player.addListener(SkipOnError(player))

        val openApp = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        session = MediaSession.Builder(this, player)
            .setSessionActivity(openApp)
            .setCallback(object : MediaSession.Callback {
                override fun onAddMediaItems(
                    mediaSession: MediaSession,
                    controller: MediaSession.ControllerInfo,
                    mediaItems: MutableList<MediaItem>,
                ): ListenableFuture<MutableList<MediaItem>> =
                    Futures.immediateFuture(mediaItems.map(::resolve).toMutableList())
            })
            .build()
    }

    /** Arquivo baixado se existir; senão streaming. Prévias do Flow sempre por streaming. */
    private fun resolve(item: MediaItem): MediaItem {
        val id = item.mediaId
        val uri: Uri? = when {
            id.startsWith(TRACK_PREFIX) -> {
                val core = Core.get()
                core.track(id.removePrefix(TRACK_PREFIX))?.let { track ->
                    val src = core.source(track)
                    src.localPath?.let { Uri.fromFile(File(it)) } ?: src.url?.let(Uri::parse)
                }
            }
            id.startsWith(FLOW_PREFIX) -> Uri.parse(Core.get().previewUrl(id.removePrefix(FLOW_PREFIX)))
            else -> item.localConfiguration?.uri
        }
        return item.buildUpon().setUri(uri).build()
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = session

    override fun onTaskRemoved(rootIntent: Intent?) {
        // Fechou o app pela lista de recentes sem nada tocando: encerra o serviço
        val player = session?.player
        if (player == null || !player.playWhenReady || player.mediaItemCount == 0) stopSelf()
    }

    override fun onDestroy() {
        tracker.release()
        session?.run {
            player.release()
            release()
        }
        session = null
        super.onDestroy()
    }

    companion object {
        const val TRACK_PREFIX = "track:"
        const val FLOW_PREFIX = "flow:"
    }
}

/**
 * Se uma faixa não carrega (ex.: caiu a internet), pula para a próxima.
 * Para depois de 3 falhas seguidas, para não ficar pulando a fila inteira.
 */
private class SkipOnError(private val player: Player) : Player.Listener {
    private var consecutive = 0

    override fun onPlayerError(error: androidx.media3.common.PlaybackException) {
        consecutive++
        if (consecutive <= 3 && player.hasNextMediaItem()) {
            player.seekToNextMediaItem()
            player.prepare()
            player.play()
        }
    }

    override fun onIsPlayingChanged(isPlaying: Boolean) {
        if (isPlaying) consecutive = 0
    }
}

/**
 * Conta um play quando a faixa da biblioteca toca por 30 s ou metade da duração (o que vier antes).
 * O play fica no aparelho e vai ao servidor na próxima sincronização.
 */
private class PlayTracker(private val player: Player) : Player.Listener {
    private val handler = Handler(Looper.getMainLooper())
    private var currentId: String? = null
    private var listenedMs = 0L
    private var counted = false

    private val tick = object : Runnable {
        override fun run() {
            if (player.isPlaying) {
                listenedMs += TICK_MS
                val duration = player.duration.takeIf { it > 0 } ?: Long.MAX_VALUE
                val id = currentId
                if (!counted && id != null && listenedMs >= minOf(30_000L, duration / 2)) {
                    counted = true
                    Core.appScope.launch { runCatching { Core.get().recordPlay(id) } }
                }
            }
            handler.postDelayed(this, TICK_MS)
        }
    }

    init {
        player.addListener(this)
        handler.postDelayed(tick, TICK_MS)
    }

    override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
        currentId = mediaItem?.mediaId?.takeIf { it.startsWith(PlaybackService.TRACK_PREFIX) }
            ?.removePrefix(PlaybackService.TRACK_PREFIX)
        listenedMs = 0
        counted = false
    }

    fun release() {
        handler.removeCallbacks(tick)
        player.removeListener(this)
    }

    companion object {
        const val TICK_MS = 1_000L
    }
}

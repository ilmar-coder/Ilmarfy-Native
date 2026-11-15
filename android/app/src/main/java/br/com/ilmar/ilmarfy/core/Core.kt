package br.com.ilmar.ilmarfy.core

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.withContext
import uniffi.ilmarfy_ffi.IlmarfyCore
import uniffi.ilmarfy_ffi.IlmarfyException

/**
 * Acesso ao núcleo em Rust (ilmarfy-core via UniFFI). As chamadas do núcleo são bloqueantes:
 * use [io] para rodá-las fora da thread de interface.
 */
object Core {
    private lateinit var appContext: Context
    private lateinit var store: TokenStore
    @Volatile private var instance: IlmarfyCore? = null

    /** Escopo do app: downloads e sincronização continuam mesmo saindo da tela */
    val appScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val _loggedIn = MutableStateFlow(false)
    val loggedIn: StateFlow<Boolean> = _loggedIn.asStateFlow()

    /** Muda a cada sincronização ou alteração na biblioteca; as telas recarregam quando muda */
    private val _version = MutableStateFlow(0)
    val version: StateFlow<Int> = _version.asStateFlow()

    fun init(context: Context) {
        appContext = context.applicationContext
        store = TokenStore(appContext)
        _loggedIn.value = store.token != null
    }

    val server: String get() = store.server
    val deviceName: String? get() = store.deviceName

    private fun open(token: String?): IlmarfyCore =
        IlmarfyCore(appContext.filesDir.absolutePath, store.server, token)

    /** Núcleo com o token salvo (criado na primeira vez) */
    fun get(): IlmarfyCore =
        instance ?: synchronized(this) { instance ?: open(store.token).also { instance = it } }

    suspend fun <T> io(block: (IlmarfyCore) -> T): T = withContext(Dispatchers.IO) { block(get()) }

    fun changed() = _version.update { it + 1 }

    suspend fun login(server: String, password: String, deviceName: String) = withContext(Dispatchers.IO) {
        store.server = server
        val core = synchronized(this@Core) { open(null).also { instance = it } }
        val token = core.login(password, deviceName)
        store.token = token
        store.deviceName = deviceName
        _loggedIn.value = true
    }

    suspend fun logout() = withContext(Dispatchers.IO) {
        runCatching { get().logout() }
        store.token = null
        synchronized(this@Core) { instance = open(null) }
        _loggedIn.value = false
    }

    /** Token revogado/inválido: volta para o login sem tentar falar com o servidor */
    fun forceLogout() {
        store.token = null
        synchronized(this) { instance = null }
        _loggedIn.value = false
    }
}

/** Mensagem amigável para mostrar ao usuário */
fun Throwable.friendly(): String = when (this) {
    is IlmarfyException.Network -> "Sem conexão com o servidor"
    is IlmarfyException.Unauthorized -> detail
    is IlmarfyException.RateLimited -> detail
    is IlmarfyException.NotFound -> detail
    is IlmarfyException.Other -> detail
    else -> message ?: "Erro inesperado"
}

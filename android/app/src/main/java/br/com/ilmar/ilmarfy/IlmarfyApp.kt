package br.com.ilmar.ilmarfy

import android.app.Application
import br.com.ilmar.ilmarfy.core.Core

class IlmarfyApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Core.init(this)
    }
}

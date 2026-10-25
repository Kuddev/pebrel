package io.github.kuddev.pebrel.mobile.ui

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeCallback
import com.journeyapps.barcodescanner.BarcodeResult
import com.journeyapps.barcodescanner.BarcodeView
import com.journeyapps.barcodescanner.CameraPreview
import com.journeyapps.barcodescanner.DefaultDecoderFactory
import com.journeyapps.barcodescanner.Size
import io.github.kuddev.pebrel.mobile.R

@Composable
internal fun InlinePairingScanner(onResult: (String) -> Unit) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val currentResult by rememberUpdatedState(onResult)
    fun permitted() = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
    var granted by remember { mutableStateOf(permitted()) }
    var openSettings by remember { mutableStateOf(false) }
    var captured by remember { mutableStateOf(false) }
    var cameraFailed by remember { mutableStateOf(false) }
    var previewReady by remember { mutableStateOf(false) }
    val hasCamera = remember { context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed ->
        granted = allowed
        openSettings = !allowed && (context as? Activity)?.shouldShowRequestPermissionRationale(Manifest.permission.CAMERA) == false
    }
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) granted = permitted()
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    val showCamera = hasCamera && granted && !captured && !cameraFailed
    val label = stringResource(R.string.pair_scan_frame)
    Box(Modifier.fillMaxWidth().heightIn(min = 258.dp).clip(RoundedCornerShape(14.dp))
        .background(MaterialTheme.colorScheme.surfaceVariant).semantics { contentDescription = label }) {
        if (showCamera) {
            val framePixels = with(LocalDensity.current) { 152.dp.roundToPx() }
            var scanner by remember { mutableStateOf<BarcodeView?>(null) }
            Column(Modifier.fillMaxWidth()) {
                Box(Modifier.fillMaxWidth().height(226.dp)) {
                    AndroidView(factory = { viewContext ->
                        BarcodeView(viewContext).apply {
                            // TextureView 才能在页内圆角区域裁切，避免相机画面盖到分段按钮上。
                            setUseTextureView(true)
                            decoderFactory = DefaultDecoderFactory(listOf(BarcodeFormat.QR_CODE))
                            framingRectSize = Size(framePixels, framePixels)
                            scanner = this
                        }
                    }, modifier = Modifier.matchParentSize(), onReset = null, onRelease = { it.stopDecoding(); it.pause() })
                    DisposableEffect(scanner, lifecycle) {
                        val view = scanner
                        var accepting = false
                        var disposed = false
                        fun stop() { accepting = false; view?.stopDecoding(); view?.pause() }
                        fun start() {
                            if (view == null || accepting) return
                            accepting = true
                            view.decodeSingle(object : BarcodeCallback {
                                override fun barcodeResult(result: BarcodeResult) {
                                    if (!accepting || disposed) return
                                    stop()
                                    captured = true
                                    currentResult(result.text)
                                }
                            })
                            view.resume()
                        }
                        view?.addStateListener(object : CameraPreview.StateListener {
                            override fun previewSized() = Unit
                            override fun previewStarted() { if (!disposed) previewReady = true }
                            override fun previewStopped() { if (!disposed) previewReady = false }
                            override fun cameraClosed() = Unit
                            override fun cameraError(error: Exception) { if (!disposed) { stop(); cameraFailed = true } }
                        })
                        val observer = LifecycleEventObserver { _, event ->
                            if (event == Lifecycle.Event.ON_RESUME) start()
                            if (event == Lifecycle.Event.ON_PAUSE || event == Lifecycle.Event.ON_STOP) stop()
                        }
                        lifecycle.addObserver(observer)
                        if (lifecycle.currentState.isAtLeast(Lifecycle.State.RESUMED)) start()
                        onDispose {
                            // 分段切换和退页都会销毁预览，同时丢弃已经排队的识别回调。
                            disposed = true
                            lifecycle.removeObserver(observer)
                            stop()
                        }
                    }
                    ScannerFrame(Modifier.matchParentSize())
                    if (!previewReady) CircularProgressIndicator(Modifier.align(Alignment.Center).size(24.dp), strokeWidth = 2.dp)
                }
                // 提示独立排版，大字体不会盖住取景边界或缩小可见二维码区域。
                Text(stringResource(R.string.pair_scan_target), color = Color.White, fontSize = 12.sp, lineHeight = 20.sp,
                    textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth()
                        .background(Color.Black).padding(horizontal = 12.dp, vertical = 6.dp))
            }
        } else Column(Modifier.fillMaxWidth().padding(horizontal = 20.dp, vertical = 36.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Glyph(R.drawable.ic_qr, Modifier.size(32.dp))
            Text(stringResource(when {
                !hasCamera -> R.string.pair_camera_missing
                captured -> R.string.pair_scan_captured
                cameraFailed -> R.string.pair_camera_failed
                else -> R.string.pair_camera_permission
            }), fontSize = 14.sp, lineHeight = 22.sp, textAlign = TextAlign.Center)
            if (hasCamera) TextButton({
                when {
                    openSettings && !granted -> context.startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                        Uri.parse("package:${context.packageName}")))
                    !granted -> permission.launch(Manifest.permission.CAMERA)
                    else -> { captured = false; cameraFailed = false; previewReady = false }
                }
            }, modifier = Modifier.heightIn(min = 48.dp)) {
                Text(stringResource(when {
                    openSettings && !granted -> R.string.pair_camera_settings
                    !granted -> R.string.pair_camera_enable
                    else -> R.string.pair_scan_retry
                }))
            }
        }
    }
}

@Composable
private fun ScannerFrame(modifier: Modifier) {
    val accent = MaterialTheme.colorScheme.primary
    Canvas(modifier) {
        val side = 152.dp.toPx().coerceAtMost(size.width - 32.dp.toPx())
        val left = (size.width - side) / 2
        val top = (size.height - side) / 2
        val shade = Color.Black.copy(alpha = .35f)
        drawRect(shade, size = androidx.compose.ui.geometry.Size(size.width, top))
        drawRect(shade, topLeft = Offset(0f, top + side), size = androidx.compose.ui.geometry.Size(size.width, top))
        drawRect(shade, topLeft = Offset(0f, top), size = androidx.compose.ui.geometry.Size(left, side))
        drawRect(shade, topLeft = Offset(left + side, top), size = androidx.compose.ui.geometry.Size(left, side))
        val arm = 26.dp.toPx()
        val stroke = 2.dp.toPx()
        for (x in listOf(left, left + side)) for (y in listOf(top, top + side)) {
            drawLine(Color.White, Offset(x, y), Offset(x + if (x == left) arm else -arm, y), stroke, StrokeCap.Round)
            drawLine(Color.White, Offset(x, y), Offset(x, y + if (y == top) arm else -arm), stroke, StrokeCap.Round)
        }
        drawLine(accent, Offset(left + 8.dp.toPx(), size.height / 2),
            Offset(left + side - 8.dp.toPx(), size.height / 2), 1.dp.toPx())
    }
}

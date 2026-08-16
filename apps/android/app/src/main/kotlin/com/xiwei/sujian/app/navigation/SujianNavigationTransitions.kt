package com.xiwei.sujian.app.navigation

import androidx.compose.animation.AnimatedContentTransitionScope
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.togetherWith
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.scene.Scene
import androidx.navigation3.ui.NavDisplay

// #614：Works/StarMap/Stats 一级入口无整页动画 — NavEntry metadata 覆盖全局 transitionSpec。
internal val noPageTransitionMetadata: Map<String, Any> =
    androidx.navigation3.ui.NavDisplay.transitionSpec {
        EnterTransition.None togetherWith ExitTransition.None
    } +
        androidx.navigation3.ui.NavDisplay.popTransitionSpec {
            EnterTransition.None togetherWith ExitTransition.None
        }

internal val navForwardTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    fadeIn(animationSpec = tween(180)) togetherWith fadeOut(animationSpec = tween(150))
}

internal val navPopTransition: AnimatedContentTransitionScope<Scene<NavKey>>.() -> ContentTransform = {
    fadeIn(animationSpec = tween(150)) togetherWith fadeOut(animationSpec = tween(180))
}

internal val navPredictivePopTransition:
    (AnimatedContentTransitionScope<Scene<NavKey>>, Int) -> ContentTransform = { _, swipeEdge ->
        when (swipeEdge) {
            androidx.navigationevent.NavigationEvent.EDGE_LEFT -> {
                val enter = slideInHorizontally(animationSpec = tween(300)) { fullWidth -> -fullWidth / 3 }
                val exit = slideOutHorizontally(animationSpec = tween(300)) { fullWidth -> fullWidth / 3 }
                (fadeIn(animationSpec = tween(300)) + enter) togetherWith
                    (fadeOut(animationSpec = tween(300)) + exit)
            }
            androidx.navigationevent.NavigationEvent.EDGE_RIGHT -> {
                val enter = slideInHorizontally(animationSpec = tween(300)) { fullWidth -> fullWidth / 3 }
                val exit = slideOutHorizontally(animationSpec = tween(300)) { fullWidth -> -fullWidth / 3 }
                (fadeIn(animationSpec = tween(300)) + enter) togetherWith
                    (fadeOut(animationSpec = tween(300)) + exit)
            }
            else -> fadeIn(animationSpec = tween(300)) togetherWith fadeOut(animationSpec = tween(300))
        }
    }

internal fun predictiveBackStateFraction(progress: Float): Float =
    PREDICTIVE_BACK_EASING.transform(progress) * SINGLE_PANE_PROGRESS_RATIO

private val PREDICTIVE_BACK_EASING: androidx.compose.animation.core.CubicBezierEasing =
    androidx.compose.animation.core.CubicBezierEasing(0.1f, 0.1f, 0f, 1f)

internal const val SINGLE_PANE_PROGRESS_RATIO: Float = 0.1f

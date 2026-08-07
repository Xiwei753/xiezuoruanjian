package com.xiwei.sujian.data.starmap

import com.xiwei.sujian.data.BridgeResult
import uniffi.writer_core.StarMapGraphDto

internal class CallCountingStarMapBridgeOps(
    private val delegate: StarMapBridgeOps,
) : StarMapBridgeOps by delegate {
    var getStarMapGraphCallCount: Int = 0
        private set

    override fun getStarMapGraph(starmapId: String): BridgeResult<StarMapGraphDto> {
        getStarMapGraphCallCount++
        return delegate.getStarMapGraph(starmapId)
    }
}

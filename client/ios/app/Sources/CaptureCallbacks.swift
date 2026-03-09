import Foundation

@_cdecl("virtue_ios_capture_status")
public func virtue_ios_capture_status() -> Int32 {
    // The app target no longer captures frames directly.
    // Safari extension provides the active capture callbacks.
    2
}

@_cdecl("virtue_ios_capture_png_write")
public func virtue_ios_capture_png_write(
    _ outBuffer: UnsafeMutablePointer<UnsafePointer<UInt8>?>?,
    _ outLength: UnsafeMutablePointer<Int>?
) -> Int32 {
    guard let outBuffer, let outLength else {
        return -1
    }
    outBuffer.pointee = nil
    outLength.pointee = 0
    return 1
}

@_cdecl("virtue_ios_capture_png_release")
public func virtue_ios_capture_png_release(_ buffer: UnsafePointer<UInt8>?, _ length: Int) {
    _ = buffer
    _ = length
}

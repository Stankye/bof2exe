# Standalone download artifacts

The BeaconDownload compatibility implementation accepts the C signature:

```c
int BeaconDownload(const char *filename, const char *buffer, unsigned int length);
```

It copies the filename and binary bytes into the executing call's output capture before returning 1. The caller may then change or free its buffer. Invalid inputs, allocation/capture limits, or a call from a thread without an active execution capture return 0. Failures produce an error callback when possible.

This is standalone local output. It does not implement a Cobalt Strike server, C2 transport, or Beacon session. The original API is documented in the [vendor BOF C API reference](https://hstechdocs.helpsystems.com/manuals/cobaltstrike/current/userguide/content/topics/beacon-object-files_bof-c-api.htm).

## Files and output

Generated CLIs and text-mode `run` save captured downloads after object execution into a freshly created `bof2exe-downloads-<unique-id>` directory under the current working directory. Each artifact has a numbered subdirectory and retains its original filename. Repeated filenames are preserved separately; existing files are not overwritten. Standard output reports each saved path and byte count. File-creation and write errors make the command fail.

JSON-mode `run` returns the captured events without writing artifact files. The reserved event kind is `i32::MIN`; its data contains the versioned download envelope, filename, and binary payload. Library clients should use `downloads::decode` to access borrowed metadata/data or `downloads::write_output` to materialize artifacts and route ordinary callbacks to stdout/stderr.

## Limits

- Filenames are UTF-8 leaf names, at most 255 bytes. Paths, control characters, Windows device names, trailing dots/spaces, and Windows-invalid filename characters are rejected.
- Empty buffers are supported. A null buffer is accepted only with zero length. Native callers must supply valid readable pointers and a NUL-terminated filename.
- Filenames, framing, downloads, and ordinary callbacks share the existing 64 MiB capture budget and 65,536-event limit. This is smaller than the original Beacon API's transfer limit.
- Capture is synchronous on the executing thread. Worker-thread download calls fail instead of claiming to have accepted an artifact that cannot be returned. The object must finish its threads before returning.
- This implementation does not reassemble application-specific multipart downloads or execute CNA orchestration. For example, BARK's split JSON files still require their original reassembly semantics.

The API queues a copied artifact; it does not guarantee that later filesystem output succeeds. Check the CLI exit status and saved artifact paths. Static compatibility alone does not prove that an arbitrary module runs correctly.

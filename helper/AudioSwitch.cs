// AudioSwitch.cs — helper for Windows Sidebar gadget "Audio Output" (8GadgetPack)
// Controls the ACTIVE playback (output) audio device:
//   list            -> dump state (devices, default, volume, mute)
//   set <index>     -> set default playback device (all 3 roles)
//   vol <0-100>     -> set default device master volume
//   mute            -> toggle default device mute
// Every invocation writes full state to %TEMP%\audioswitch_out.txt (UTF-8 BOM)
// and also prints it to stdout. Gadget JS reads the file (avoids shell quoting hell).

using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

class AudioSwitch
{
    const string FILE = "audioswitch_out.txt";
    const uint ACTIVE = 1; // DEVICE_STATE_ACTIVE

    #region COM interop

    public enum EDataFlow { eRender = 0, eCapture = 1, eAll = 2 }
    public enum ERole { eConsole = 0, eMultimedia = 1, eCommunications = 2 }

    [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")]
    private class MMDeviceEnumeratorCom { }

    [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMMDeviceEnumerator
    {
        [PreserveSig] int EnumAudioEndpoints(EDataFlow dataFlow, uint dwStateMask, out IMMDeviceCollection ppDevices);
        [PreserveSig] int GetDefaultAudioEndpoint(EDataFlow dataFlow, ERole role, out IMMDevice ppDevice);
    }

    [ComImport, Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMMDeviceCollection
    {
        [PreserveSig] int GetCount(out uint pcDevices);
        [PreserveSig] int Item(uint nDevice, out IMMDevice ppDevice);
    }

    [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IMMDevice
    {
        [PreserveSig] int Activate([In] ref Guid iid, uint dwClsCtx, IntPtr pActivationParams,
            [Out, MarshalAs(UnmanagedType.IUnknown)] out object ppInterface);
        [PreserveSig] int OpenPropertyStore(uint stgmAccess, [Out] out IPropertyStore ppProperties);
        [PreserveSig] int GetId([MarshalAs(UnmanagedType.LPWStr)] out string ppstrId);
        [PreserveSig] int GetState(out uint pdwState);
    }

    [ComImport, Guid("886d8eeb-8cf2-4446-8d02-cdba1dbdcf99"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPropertyStore
    {
        [PreserveSig] int GetCount(out uint cProps);
        [PreserveSig] int GetAt(uint iProp, out PROPERTYKEY pkey);
        [PreserveSig] int GetValue([In] ref PROPERTYKEY key, out PROPVARIANT pv);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PROPERTYKEY { public Guid fmtid; public int pid; }

    [StructLayout(LayoutKind.Sequential)]
    private struct PROPVARIANT
    {
        public ushort vt;
        public ushort r1, r2, r3;
        public IntPtr p;
        public int p2;
    }

    [DllImport("ole32.dll")]
    private static extern int PropVariantClear(ref PROPVARIANT pvar);

    private static readonly PROPERTYKEY PKEY_Device_FriendlyName =
        new PROPERTYKEY { fmtid = new Guid("a45c254e-df1c-4efd-8020-67d146a850e0"), pid = 14 };

    [ComImport, Guid("5CDF2C82-841E-4546-9722-0CF74078229A"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IAudioEndpointVolume
    {
        [PreserveSig] int RegisterControlChangeNotify(IntPtr n);
        [PreserveSig] int UnregisterControlChangeNotify(IntPtr n);
        [PreserveSig] int GetChannelCount(out uint c);
        [PreserveSig] int SetMasterVolumeLevel(float fLevelDB, ref Guid ctx);
        [PreserveSig] int SetMasterVolumeLevelScalar(float fLevel, ref Guid ctx);
        [PreserveSig] int GetMasterVolumeLevel(out float f);
        [PreserveSig] int GetMasterVolumeLevelScalar(out float f);
        void _SetChannelVolumeLevel();
        void _SetChannelVolumeLevelScalar();
        void _GetChannelVolumeLevel();
        void _GetChannelVolumeLevelScalar();
        [PreserveSig] int SetMute([MarshalAs(UnmanagedType.Bool)] bool bMute, ref Guid ctx);
        [PreserveSig] int GetMute([MarshalAs(UnmanagedType.Bool)] out bool bMute);
    }

    // undocumented but stable (Win7 -> Win11), used by SoundSwitch / EarTrumpet etc.
    [ComImport, Guid("f8679f50-850a-41cf-9c72-430f290290c8"),
     InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IPolicyConfig
    {
        [PreserveSig] int GetMixFormat(string d, IntPtr f);
        [PreserveSig] int GetDeviceFormat(string d, [MarshalAs(UnmanagedType.Bool)] bool def, IntPtr f);
        [PreserveSig] int ResetDeviceFormat(string d);
        [PreserveSig] int SetDeviceFormat(string d, IntPtr e, IntPtr m);
        [PreserveSig] int GetProcessingPeriod(string d, [MarshalAs(UnmanagedType.Bool)] bool def, IntPtr a, IntPtr b);
        [PreserveSig] int SetProcessingPeriod(string d, IntPtr t);
        [PreserveSig] int GetShareMode(string d, IntPtr m);
        [PreserveSig] int SetShareMode(string d, IntPtr m);
        [PreserveSig] int GetPropertyValue(string d, [MarshalAs(UnmanagedType.Bool)] bool fx, IntPtr k, IntPtr v);
        [PreserveSig] int SetPropertyValue(string d, [MarshalAs(UnmanagedType.Bool)] bool fx, IntPtr k, IntPtr v);
        [PreserveSig] int SetDefaultEndpoint(string d, int role);
    }

    [ComImport, Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")]
    private class PolicyConfigClientCom { }

    #endregion

    static string GetId(IMMDevice d)
    {
        string id;
        d.GetId(out id);
        return id;
    }

    static string GetName(IMMDevice d)
    {
        IPropertyStore ps;
        int hr = d.OpenPropertyStore(0, out ps); // STGM_READ
        Marshal.ThrowExceptionForHR(hr);
        PROPERTYKEY key = PKEY_Device_FriendlyName;
        PROPVARIANT pv;
        hr = ps.GetValue(ref key, out pv);
        Marshal.ThrowExceptionForHR(hr);
        string s = null;
        try { if (pv.vt == 31) s = Marshal.PtrToStringUni(pv.p); } // VT_LPWSTR
        finally { PropVariantClear(ref pv); }
        if (string.IsNullOrEmpty(s)) s = "(unnamed device)";
        return s.Replace('\r', ' ').Replace('\n', ' ');
    }

    static IAudioEndpointVolume Vol(IMMDevice d)
    {
        Guid iid = new Guid("5CDF2C82-841E-4546-9722-0CF74078229A");
        object o;
        int hr = d.Activate(ref iid, 1 /*CLSCTX_INPROC_SERVER*/, IntPtr.Zero, out o);
        Marshal.ThrowExceptionForHR(hr);
        return (IAudioEndpointVolume)o;
    }

    static IMMDeviceEnumerator Enumerator()
    {
        return (IMMDeviceEnumerator)new MMDeviceEnumeratorCom();
    }

    static List<IMMDevice> RenderDevices(IMMDeviceEnumerator en)
    {
        IMMDeviceCollection coll;
        int hr = en.EnumAudioEndpoints(EDataFlow.eRender, ACTIVE, out coll);
        Marshal.ThrowExceptionForHR(hr);
        uint n;
        coll.GetCount(out n);
        List<IMMDevice> list = new List<IMMDevice>((int)n);
        for (uint i = 0; i < n; i++)
        {
            IMMDevice d;
            coll.Item(i, out d);
            list.Add(d);
        }
        return list;
    }

    static IMMDevice DefaultDevice(IMMDeviceEnumerator en)
    {
        IMMDevice t;
        int hr = en.GetDefaultAudioEndpoint(EDataFlow.eRender, ERole.eConsole, out t);
        if (hr != 0) throw new InvalidOperationException("no default playback device");
        return t;
    }

    static void SetDefault(int idx)
    {
        IMMDeviceEnumerator en = Enumerator();
        List<IMMDevice> devs = RenderDevices(en);
        if (idx < 1 || idx > devs.Count) throw new ArgumentException("bad device index: " + idx);
        string id = GetId(devs[idx - 1]);
        IPolicyConfig pc = (IPolicyConfig)new PolicyConfigClientCom();
        Marshal.ThrowExceptionForHR(pc.SetDefaultEndpoint(id, 0)); // eConsole
        Marshal.ThrowExceptionForHR(pc.SetDefaultEndpoint(id, 1)); // eMultimedia
        Marshal.ThrowExceptionForHR(pc.SetDefaultEndpoint(id, 2)); // eCommunications
    }

    static void SetVolume(double pct)
    {
        IMMDeviceEnumerator en = Enumerator();
        IAudioEndpointVolume v = Vol(DefaultDevice(en));
        float f = (float)Math.Max(0.0, Math.Min(100.0, pct)) / 100f;
        Guid g = Guid.Empty;
        Marshal.ThrowExceptionForHR(v.SetMasterVolumeLevelScalar(f, ref g));
    }

    static void ToggleMute()
    {
        IMMDeviceEnumerator en = Enumerator();
        IAudioEndpointVolume v = Vol(DefaultDevice(en));
        bool m;
        Marshal.ThrowExceptionForHR(v.GetMute(out m));
        Guid g = Guid.Empty;
        Marshal.ThrowExceptionForHR(v.SetMute(!m, ref g));
    }

    static string BuildState()
    {
        IMMDeviceEnumerator en = Enumerator();
        List<IMMDevice> devs = RenderDevices(en);

        IMMDevice def = null;
        string defId = null;
        try { def = DefaultDevice(en); defId = GetId(def); }
        catch { }

        StringBuilder sb = new StringBuilder();
        int defIdx = 0;
        for (int i = 0; i < devs.Count; i++)
        {
            if (defIdx == 0 && defId != null && GetId(devs[i]) == defId) defIdx = i + 1;
            sb.Append("DEV|").Append((i + 1).ToString(CultureInfo.InvariantCulture))
              .Append('|').Append(GetName(devs[i])).Append('\n');
        }
        sb.Append("DEFAULT|").Append(defIdx.ToString(CultureInfo.InvariantCulture)).Append('\n');

        int vol = -1, mute = -1;
        if (def != null)
        {
            try
            {
                IAudioEndpointVolume v = Vol(def);
                float s; v.GetMasterVolumeLevelScalar(out s);
                vol = (int)Math.Round(s * 100.0);
                bool m; v.GetMute(out m);
                mute = m ? 1 : 0;
            }
            catch { }
        }
        sb.Append("VOLUME|").Append(vol.ToString(CultureInfo.InvariantCulture))
          .Append('|').Append(mute.ToString(CultureInfo.InvariantCulture)).Append('\n');
        return sb.ToString();
    }

    static void Main(string[] args)
    {
        string outPath = Path.Combine(Path.GetTempPath(), FILE);
        string body;
        try
        {
            string cmd = (args != null && args.Length > 0) ? args[0].ToLowerInvariant() : "list";
            if (cmd == "set")
            {
                if (args.Length < 2) throw new ArgumentException("set <index>");
                SetDefault(int.Parse(args[1], CultureInfo.InvariantCulture));
            }
            else if (cmd == "vol")
            {
                if (args.Length < 2) throw new ArgumentException("vol <0-100>");
                SetVolume(double.Parse(args[1], CultureInfo.InvariantCulture));
            }
            else if (cmd == "mute") ToggleMute();
            else if (cmd != "list") throw new ArgumentException("unknown command: " + cmd);
            body = BuildState();
        }
        catch (Exception ex)
        {
            string m = ex.Message == null ? "error" : ex.Message.Replace("\r", " ").Replace("\n", " ");
            body = "ERROR|" + m;
        }
        File.WriteAllText(outPath, body, Encoding.Unicode);
        try { Console.OutputEncoding = Encoding.UTF8; } catch { }
        Console.Write(body);
    }
}
